//! Read-only native storage inspection. Reported history is never recovery authorization.
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::Instant,
};

const MAX_SESSION_BYTES: u64 = 128 * 1024 * 1024;
const MAX_RECORD_BYTES: u64 = 1024 * 1024;
const MAX_RUNS: usize = 256;
const MAX_RUN_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct Inspection {
    pub schema: u32,
    pub session: Session,
    pub delegated: Delegated,
    pub eligible: bool,
    pub missing_evidence: Vec<&'static str>,
    pub files_read: usize,
    pub bytes_read: u64,
    pub elapsed_micros: u128,
}

#[derive(Debug, Serialize)]
pub struct Session {
    pub native_id: String,
    pub file: PathBuf,
    pub cwd: PathBuf,
    pub device: u64,
    pub inode: u64,
    /// This is NOT necessarily the runtime's active branch.
    pub last_persisted_entry: Option<String>,
    pub last_persisted_message_role: Option<String>,
    pub entries: usize,
    pub last_reported_assistant_stop_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Delegated {
    /// Disk async status excludes foreground runs and is never exhaustive.
    pub coverage: &'static str,
    pub truncated: bool,
    pub runs: Vec<Run>,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Serialize)]
pub struct Run {
    pub run_id: String,
    /// pi-subagents prefers the full session-file path, with native ID fallback.
    pub reported_parent_session_ref: String,
    pub reported_state: String,
    pub attribution_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct Warning {
    pub path: Option<PathBuf>,
    pub reason: &'static str,
}

pub(crate) fn open_source(path: &Path, max: u64) -> Result<File> {
    crate::canonical(path)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    ensure!(metadata.is_file(), "source must be a regular file");
    ensure!(
        metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o022 == 0,
        "source must be owned by the current user and not group/other writable"
    );
    ensure!(
        metadata.len() <= max,
        "source exceeds inspection byte limit"
    );
    Ok(file)
}

pub(crate) fn unchanged(path: &Path, file: &File, before: &fs::Metadata) -> Result<()> {
    fn signature(meta: &fs::Metadata) -> (u64, u64, u64, i64, i64, i64, i64) {
        (
            meta.dev(),
            meta.ino(),
            meta.len(),
            meta.mtime(),
            meta.mtime_nsec(),
            meta.ctime(),
            meta.ctime_nsec(),
        )
    }
    crate::canonical(path)?;
    let after = file.metadata()?;
    let named = fs::symlink_metadata(path)?;
    ensure!(
        named.is_file()
            && signature(before) == signature(&after)
            && signature(before) == signature(&named),
        "source changed during inspection"
    );
    Ok(())
}

fn header_identity(header: &Value) -> Result<(String, PathBuf)> {
    ensure!(
        header["type"] == "session" && header["version"] == 3,
        "unsupported Pi session header"
    );
    let id = header["id"].as_str().context("session has no ID")?;
    ensure!(crate::identifier(id), "invalid session ID");
    let cwd = PathBuf::from(header["cwd"].as_str().context("session has no cwd")?);
    ensure!(cwd.is_absolute(), "session cwd must be absolute");
    Ok((id.to_owned(), cwd))
}

/// Policy needs identity only, not a complete history or an existing workspace.
/// Read a bounded complete header; appending history must not prevent disabling.
pub(crate) fn native_identity(path: &Path) -> Result<crate::Identity> {
    let file = open_source(path, u64::MAX)?;
    let mut line = Vec::new();
    BufReader::new(file)
        .take(MAX_RECORD_BYTES + 1)
        .read_until(b'\n', &mut line)?;
    ensure!(
        line.len() as u64 <= MAX_RECORD_BYTES,
        "session header exceeds byte limit"
    );
    ensure!(line.ends_with(b"\n"), "incomplete session header");
    let header: Value = serde_json::from_slice(&line).context("invalid session header JSON")?;
    let (session_id, cwd) = header_identity(&header)?;
    Ok(crate::Identity {
        harness: "pi".into(),
        session_id,
        session_file: path.to_owned(),
        cwd,
        leaf: None,
    })
}

fn session(
    path: &Path,
    require_workspace: bool,
    mut evidence: Option<&mut crate::pi_evidence::Index>,
    required_leaf: Option<&str>,
    max_bytes: u64,
) -> Result<(Session, u64)> {
    let max_bytes = max_bytes.min(MAX_SESSION_BYTES);
    let file = open_source(path, max_bytes)?;
    let metadata = file.metadata()?;
    let mut reader = BufReader::new(file);
    let mut bytes = 0;
    let mut line = Vec::new();
    let mut report: Option<Session> = None;
    let mut found_leaf = required_leaf.is_none();
    loop {
        line.clear();
        let count = (&mut reader)
            .take((MAX_RECORD_BYTES + 1).min(max_bytes.saturating_sub(bytes) + 1))
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            break;
        }
        ensure!(
            line.len() as u64 <= MAX_RECORD_BYTES,
            "session entry exceeds inspection byte limit"
        );
        bytes += count as u64;
        ensure!(bytes <= max_bytes, "session grew beyond byte limit");
        ensure!(line.ends_with(b"\n"), "incomplete session write");
        let entry: Value = serde_json::from_slice(&line).context("invalid session JSON")?;
        if let Some(report) = &mut report {
            ensure!(entry["type"] != "session", "duplicate session header");
            let id = entry["id"].as_str().context("entry has no ID")?;
            ensure!(crate::identifier(id), "invalid entry ID");
            found_leaf |= required_leaf == Some(id);
            report.last_persisted_entry = Some(id.to_owned());
            report.last_persisted_message_role = if entry["type"] == "message" {
                entry["message"]["role"]
                    .as_str()
                    .filter(|role| matches!(*role, "user" | "assistant" | "toolResult"))
                    .map(str::to_owned)
            } else {
                None
            };
            report.entries += 1;
            if let Some(index) = evidence.as_deref_mut() {
                index.observe(&entry)?;
            }
            if entry["type"] == "message" && entry["message"]["role"] == "assistant" {
                report.last_reported_assistant_stop_reason = entry["message"]["stopReason"]
                    .as_str()
                    .filter(|reason| {
                        matches!(*reason, "stop" | "length" | "toolUse" | "error" | "aborted")
                    })
                    .map(str::to_owned);
            }
        } else {
            let (id, cwd) = header_identity(&entry)?;
            if require_workspace {
                crate::canonical(&cwd)?;
                ensure!(cwd.is_dir(), "session cwd must be a directory");
            }
            report = Some(Session {
                native_id: id,
                file: path.to_owned(),
                cwd,
                device: metadata.dev(),
                inode: metadata.ino(),
                last_persisted_entry: None,
                last_persisted_message_role: None,
                entries: 0,
                last_reported_assistant_stop_reason: None,
            });
        }
    }
    unchanged(path, reader.get_ref(), &metadata)?;
    ensure!(found_leaf, "branch leaf absent from session file");
    Ok((report.context("empty session file")?, bytes))
}

/// Apply the same bounded, stable-file validation to retained legacy identities.
pub(crate) fn registered_session(identity: &crate::Identity) -> Result<Session> {
    Ok(session(
        &identity.session_file,
        true,
        None,
        identity.leaf.as_deref(),
        MAX_SESSION_BYTES,
    )?
    .0)
}

fn status(path: &Path, files_read: &mut usize, bytes_read: &mut u64) -> Result<Value> {
    let mut file = open_source(path, MAX_RECORD_BYTES)?;
    *files_read += 1;
    let metadata = file.metadata()?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)?;
    *bytes_read += bytes.len() as u64;
    ensure!(
        bytes.len() as u64 <= MAX_RECORD_BYTES,
        "status grew beyond byte limit"
    );
    unchanged(path, &file, &metadata)?;
    serde_json::from_slice(&bytes).context("invalid run status JSON")
}

fn delegated(root: Option<&Path>, session: &Session) -> Result<(Delegated, usize, u64)> {
    let mut report = Delegated {
        coverage: "unavailable",
        truncated: false,
        runs: Vec::new(),
        warnings: Vec::new(),
    };
    let Some(root) = root else {
        report.warnings.push(Warning {
            path: None,
            reason: "no_subagents_root_selected",
        });
        return Ok((report, 0, 0));
    };
    crate::canonical(root)?;
    ensure!(root.is_dir(), "subagents root must be a directory");
    let runs_dir = root.join("async-subagent-runs");
    if !runs_dir.exists() {
        report.warnings.push(Warning {
            path: Some(runs_dir),
            reason: "async_run_storage_unavailable",
        });
        return Ok((report, 0, 0));
    }
    crate::canonical(&runs_dir)?;
    let mut paths = fs::read_dir(&runs_dir)?
        .take(MAX_RUNS + 1)
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    report.truncated = paths.len() > MAX_RUNS;
    paths.truncate(MAX_RUNS);
    paths.sort();
    report.coverage = "partial";
    let mut files_read = 0;
    let mut bytes_read = 0;
    for dir in paths {
        if bytes_read >= MAX_RUN_BYTES {
            report.truncated = true;
            report.warnings.push(Warning {
                path: None,
                reason: "run_inspection_byte_limit",
            });
            break;
        }
        let path = dir.join("status.json");
        // Reject directory symlinks before touching their target.
        if !fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
            report.warnings.push(Warning {
                path: Some(dir),
                reason: "invalid_run_directory",
            });
            continue;
        }
        let Ok(value) = status(&path, &mut files_read, &mut bytes_read) else {
            report.warnings.push(Warning {
                path: Some(path),
                reason: "unreadable_or_invalid_run_status",
            });
            continue;
        };
        let Some(parent_ref) = value["sessionId"].as_str() else {
            report.warnings.push(Warning {
                path: Some(path),
                reason: "parent_session_reference_unavailable",
            });
            continue;
        };
        if parent_ref != session.native_id && Some(parent_ref) != session.file.to_str() {
            continue;
        }
        let Some(id) = value["runId"].as_str().filter(|id| crate::identifier(id)) else {
            report.warnings.push(Warning {
                path: Some(path),
                reason: "invalid_run_identity",
            });
            continue;
        };
        if dir.file_name().and_then(|name| name.to_str()) != Some(id) {
            report.warnings.push(Warning {
                path: Some(path),
                reason: "run_directory_identity_mismatch",
            });
            continue;
        }
        let state = value["state"]
            .as_str()
            .filter(|state| {
                matches!(
                    *state,
                    "queued"
                        | "running"
                        | "complete"
                        | "failed"
                        | "partial"
                        | "paused"
                        | "stopped"
                        | "rejected"
                )
            })
            .unwrap_or("unknown");
        report.runs.push(Run {
            run_id: id.to_owned(),
            reported_parent_session_ref: parent_ref.to_owned(),
            reported_state: state.to_owned(),
            attribution_verified: false,
        });
    }
    Ok((report, files_read, bytes_read))
}

/// Inspect only the selected files; do not create recovery state, reconcile,
/// signal processes, load extensions, or launch the harness.
pub fn inspect(path: &Path, subagents_root: Option<&Path>) -> Result<Inspection> {
    inspect_with(path, subagents_root, true, None, MAX_SESSION_BYTES)
}

pub(crate) fn inspect_for_assessment(
    path: &Path,
    subagents_root: Option<&Path>,
) -> Result<(Inspection, crate::pi_evidence::PersistedLineage)> {
    let mut index = crate::pi_evidence::Index::default();
    let inspection = inspect_with(
        path,
        subagents_root,
        false,
        Some(&mut index),
        MAX_SESSION_BYTES,
    )?;
    Ok((inspection, index.finish()))
}

/// Discovery reserves this file's observed size against a host-wide read budget.
pub(crate) fn inspect_for_discovery(
    path: &Path,
    max_bytes: u64,
) -> Result<(Inspection, crate::pi_evidence::PersistedLineage)> {
    let mut index = crate::pi_evidence::Index::default();
    let inspection = inspect_with(path, None, false, Some(&mut index), max_bytes)?;
    Ok((inspection, index.finish()))
}

fn inspect_with(
    path: &Path,
    subagents_root: Option<&Path>,
    require_workspace: bool,
    evidence: Option<&mut crate::pi_evidence::Index>,
    max_bytes: u64,
) -> Result<Inspection> {
    let start = Instant::now();
    let (session, session_bytes) = session(path, require_workspace, evidence, None, max_bytes)?;
    let (delegated, run_files, run_bytes) = delegated(subagents_root, &session)?;
    Ok(Inspection {
        schema: 1,
        session,
        delegated,
        eligible: false,
        missing_evidence: vec![
            "authoritative_active_branch",
            "execution_scope",
            "ordered_lifecycle_and_approval_state",
            "host_boot_and_exclusive_owner",
            "restore_authorization_and_attempt_state",
        ],
        files_read: 1 + run_files,
        bytes_read: session_bytes + run_bytes,
        elapsed_micros: start.elapsed().as_micros(),
    })
}
