//! Bounded current-user Pi discovery. No enrollment, launch or recovery authorization.
use crate::{Record, eligibility, inspection, pi_gate, policy::Policy};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::Instant,
};

const MAX_ROOTS: usize = 16;
const MAX_ENTRIES: usize = 4096;
const MAX_DIRECTORIES: usize = 512;
const MAX_FILES: usize = 256;
const MAX_DEPTH: usize = 8;
const MAX_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
pub struct Discovery {
    pub schema: u32,
    pub harness: String,
    pub roots: Vec<PathBuf>,
    pub coverage: String,
    pub truncated: bool,
    pub boot_change_verified: bool,
    pub automatic_recovery_implemented: bool,
    pub findings: Vec<Finding>,
    pub warnings: Vec<Warning>,
    pub distinct_native_ids: usize,
    pub directories_read: usize,
    pub directory_entries_seen: usize,
    pub session_bytes_reserved: u64,
    pub elapsed_micros: u128,
    pub state_schema: u32,
    pub migration_pending: bool,
}
#[derive(Serialize, Deserialize)]
pub struct Finding {
    pub file: PathBuf,
    pub native_id: Option<String>,
    pub cwd: Option<PathBuf>,
    pub category: String,
    pub explanation: String,
    pub gate_decision: String,
    pub signal_codes: Vec<String>,
    pub shared_native_identity: bool,
}
#[derive(Serialize, Deserialize)]
pub struct Warning {
    pub path: PathBuf,
    pub code: String,
}

fn location(value: &std::ffi::OsStr, home: &Path) -> Result<PathBuf> {
    let path = PathBuf::from(value);
    let path = if path == Path::new("~") {
        home.to_owned()
    } else if let Ok(rest) = path.strip_prefix("~") {
        home.join(rest)
    } else {
        path
    };
    Ok(if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    })
}

/// Standard native storage plus an existing environment-selected session directory.
/// Project/CLI-specific locations cannot be inferred; callers may supply roots.
pub fn default_roots() -> Result<Vec<PathBuf>> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    ensure!(
        home.is_absolute(),
        "Set HOME or supply --sessions-root to find saved sessions"
    );
    let agent = match std::env::var_os("PI_CODING_AGENT_DIR").filter(|value| !value.is_empty()) {
        Some(value) => location(&value, &home)?,
        None => home.join(".pi/agent"),
    };
    let mut roots = vec![agent.join("sessions")];
    if let Some(value) =
        std::env::var_os("PI_CODING_AGENT_SESSION_DIR").filter(|value| !value.is_empty())
    {
        roots.push(location(&value, &home)?);
    }
    roots.sort();
    roots.dedup();
    Ok(roots)
}

struct Walk {
    files: BTreeSet<PathBuf>,
    visited: BTreeSet<PathBuf>,
    warnings: Vec<Warning>,
    entries: usize,
    directories: usize,
    truncated: bool,
}
impl Walk {
    fn warn(&mut self, path: &Path, code: &str) {
        self.warnings.push(Warning {
            path: path.to_owned(),
            code: code.into(),
        });
    }
    fn visit(&mut self, path: &Path, depth: usize) {
        if self.entries >= MAX_ENTRIES || self.files.len() > MAX_FILES {
            self.truncated = true;
            self.warn(path, "directory_entry_limit");
            return;
        }
        if depth > MAX_DEPTH || self.directories >= MAX_DIRECTORIES {
            self.truncated = true;
            self.warn(path, "directory_or_depth_limit");
            return;
        }
        if !self.visited.insert(path.to_owned()) {
            return;
        }
        let Ok(before) = fs::symlink_metadata(path) else {
            self.warn(path, "directory_missing_or_unreadable");
            return;
        };
        if !before.is_dir()
            || before.uid() != unsafe { libc::geteuid() }
            || before.mode() & 0o022 != 0
            || crate::canonical(path).is_err()
        {
            self.warn(path, "unsafe_or_noncanonical_directory");
            return;
        }
        let Ok(entries) = fs::read_dir(path) else {
            self.warn(path, "directory_missing_or_unreadable");
            return;
        };
        self.directories += 1;
        let remaining = MAX_ENTRIES - self.entries;
        let entries: Vec<_> = entries.take(remaining + 1).collect();
        if entries.len() > remaining {
            self.truncated = true;
            self.warn(path, "directory_entry_limit");
        }
        self.entries += entries.len().min(remaining);
        let mut children = Vec::new();
        for entry in entries.into_iter().take(remaining) {
            match entry {
                Ok(entry) => children.push(entry.path()),
                Err(_) => self.warn(path, "directory_entry_unreadable"),
            }
        }
        children.sort();
        for child in children {
            let Ok(meta) = fs::symlink_metadata(&child) else {
                self.warn(&child, "directory_entry_unreadable");
                continue;
            };
            if meta.file_type().is_symlink() {
                self.warn(&child, "symlink_not_followed");
            } else if meta.is_dir() {
                self.visit(&child, depth + 1);
            } else if child
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            {
                self.files.insert(child);
                if self.files.len() > MAX_FILES {
                    self.truncated = true;
                    self.warn(path, "session_file_limit");
                    break;
                }
            }
        }
        if !fs::symlink_metadata(path).is_ok_and(|after| {
            after.is_dir()
                && before.dev() == after.dev()
                && before.ino() == after.ino()
                && before.mtime() == after.mtime()
                && before.mtime_nsec() == after.mtime_nsec()
                && before.ctime() == after.ctime()
                && before.ctime_nsec() == after.ctime_nsec()
        }) {
            self.warn(path, "directory_changed_during_discovery");
        }
    }
}

pub(crate) fn discover(
    mut roots: Vec<PathBuf>,
    policy: &Policy,
    records: &BTreeMap<String, Record>,
    boot: &str,
    state_schema: u32,
) -> Result<Discovery> {
    ensure!(
        !roots.is_empty() && roots.len() <= MAX_ROOTS,
        "Discovery needs 1 to 16 session directories"
    );
    ensure!(
        roots.iter().all(|root| root.is_absolute()),
        "Session directories must be absolute paths"
    );
    roots.sort();
    roots.dedup();
    let start = Instant::now();
    let mut walk = Walk {
        files: BTreeSet::new(),
        visited: BTreeSet::new(),
        warnings: Vec::new(),
        entries: 0,
        directories: 0,
        truncated: false,
    };
    for root in &roots {
        walk.visit(root, 0);
    }
    let mut findings = Vec::new();
    let mut bytes_reserved = 0;
    for file in walk.files.iter().take(MAX_FILES) {
        let mut finding = Finding {
            file: file.clone(),
            native_id: None,
            cwd: None,
            category: "unreadable".into(),
            explanation: "Cannot safely assess this saved file; nothing will be restarted.".into(),
            gate_decision: "hold".into(),
            signal_codes: Vec::new(),
            shared_native_identity: false,
        };
        let size = fs::symlink_metadata(file)
            .ok()
            .filter(|meta| meta.is_file())
            .map(|meta| meta.len());
        if let Some(size) = size.filter(|size| *size <= MAX_BYTES - bytes_reserved) {
            // Reserve even failed reads. The inspector allows at most this size plus
            // one overflow-detection byte, and rejects unstable sources.
            bytes_reserved += size;
            if let Ok((inspection, lineage)) = inspection::inspect_for_discovery(file, size) {
                let report = eligibility::assess(
                    inspection,
                    lineage,
                    pi_gate::UsageEvidence {
                        coverage: "not_selected",
                        ..Default::default()
                    },
                    policy,
                    records.values(),
                    boot,
                    state_schema,
                );
                finding.native_id = Some(report.session.native_id);
                finding.cwd = Some(report.session.cwd);
                finding.gate_decision = report.recovery_gate.decision.into();
                finding.signal_codes = report
                    .recovery_gate
                    .signals
                    .iter()
                    .map(|reason| reason.code.into())
                    .collect();
                let work = report.persisted_lineage.reported_work;
                let (category, explanation) = if work.aborted_messages > 0 {
                    (
                        "recorded_cancellation",
                        "Saved history reports cancellation. Do not automatically restart that recorded work.",
                    )
                } else if !report.policy_allowed {
                    (
                        "recorded_blocker",
                        "Recovery is disabled for this host or session.",
                    )
                } else if !report.retained_records.is_empty() && finding.gate_decision == "blocked"
                {
                    (
                        "recorded_blocker",
                        "Saved recovery state blocks restarting this work; previous attempts and uncertainty remain retained.",
                    )
                } else if work.error_messages > 0 {
                    (
                        "recorded_blocker",
                        "Saved history reports an error. Your decision is needed before further work.",
                    )
                } else if finding.gate_decision == "blocked" {
                    (
                        "recorded_blocker",
                        "The saved response ended or may be waiting for you. No follow-up will be sent.",
                    )
                } else {
                    (
                        "needs_review",
                        "The records do not establish safe interrupted work. Your decision is needed.",
                    )
                };
                finding.category = category.into();
                finding.explanation = explanation.into();
            }
        } else if size.is_some() {
            walk.truncated = true;
            walk.warnings.push(Warning {
                path: file.clone(),
                code: "aggregate_session_byte_limit".into(),
            });
        }
        findings.push(finding);
    }
    let mut ids = BTreeMap::<String, usize>::new();
    for finding in &findings {
        if let Some(id) = &finding.native_id {
            *ids.entry(id.clone()).or_default() += 1;
        }
    }
    for finding in &mut findings {
        finding.shared_native_identity = finding.native_id.as_ref().is_some_and(|id| ids[id] > 1);
    }
    Ok(Discovery {
        schema: 1,
        harness: "pi".into(),
        roots,
        coverage: "bounded_selected_roots_not_all_host_storage_or_live_state".into(),
        truncated: walk.truncated,
        boot_change_verified: false,
        automatic_recovery_implemented: false,
        findings,
        warnings: walk.warnings,
        distinct_native_ids: ids.len(),
        directories_read: walk.directories,
        directory_entries_seen: walk.entries,
        session_bytes_reserved: bytes_reserved,
        elapsed_micros: start.elapsed().as_micros(),
        state_schema,
        migration_pending: state_schema == 1,
    })
}

fn printable(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .flat_map(char::escape_default)
        .collect()
}
impl Discovery {
    pub fn plain_text(&self) -> String {
        let mut text = format!(
            "Found {} saved Pi files ({} identified sessions). Nothing was restarted.\n",
            self.findings.len(),
            self.distinct_native_ids
        );
        let count = |category| {
            self.findings
                .iter()
                .filter(|finding| finding.category == category)
                .count()
        };
        text.push_str(&format!("Recorded cancellations: {}; other blockers: {}; needs your review: {}; could not safely read: {}.\n",
            count("recorded_cancellation"), count("recorded_blocker"), count("needs_review"), count("unreadable")));
        text.push_str("This reads saved records only; it does not prove a restart occurred or that work is safe to continue.\n");
        if self.findings.is_empty() {
            text.push_str("No session files were found in the searched directories. This does not prove there was no interrupted work.\n");
        }
        for finding in &self.findings {
            text.push_str(&format!(
                "\n{}\n  {}\n",
                printable(&finding.file),
                finding.explanation
            ));
            if finding.shared_native_identity {
                text.push_str("  Another saved file refers to the same session; it will not be treated as a separate job.\n");
            }
        }
        if self.truncated {
            text.push_str("\nThe search reached a safety limit; some records were not checked.\n");
        }
        for warning in &self.warnings {
            let message = match warning.code.as_str() {
                "symlink_not_followed" => "Skipped a symbolic link",
                "unsafe_or_noncanonical_directory" => {
                    "Skipped a directory with unsafe permissions or a redirected path"
                }
                "directory_missing_or_unreadable" => "Could not read a directory",
                "directory_changed_during_discovery" => "A directory changed during the search",
                "aggregate_session_byte_limit" => "Skipped a file beyond the total read limit",
                "session_file_limit" | "directory_entry_limit" | "directory_or_depth_limit" => {
                    "Search safety limit reached"
                }
                _ => "Could not safely read a directory entry",
            };
            text.push_str(&format!("{}: {}\n", message, printable(&warning.path)));
        }
        text.push_str("\nSearch covers selected directories for this user, not every account, custom location or remote child agent. No sessions were registered or given permission to run.\n");
        text
    }
}
