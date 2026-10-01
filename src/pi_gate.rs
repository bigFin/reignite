//! Negative recovery gate. Reported stops veto; missing stops never authorize work.
use crate::{
    eligibility::Reason,
    inspection::{self, Session},
    pi_evidence::PersistedLineage,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Read},
    path::Path,
    time::Instant,
};

const MAX_BYTES: u64 = 32 * 1024 * 1024;
const MAX_RECORD_BYTES: u64 = 64 * 1024;
const MAX_RECORDS: usize = 65_536;

#[derive(Default, Serialize)]
pub struct UsageEvidence {
    pub selected: bool,
    pub coverage: &'static str,
    pub records_read: usize,
    pub matching_main_records: usize,
    pub matching_child_records: usize,
    pub main_cancellations: usize,
    pub main_errors: usize,
    pub main_turn_boundaries: usize,
    pub child_cancellations: usize,
    pub identity_conflicts: usize,
    pub unattributed_records: usize,
    pub unsupported_records: usize,
    pub files_read: usize,
    pub bytes_read: u64,
    pub elapsed_micros: u128,
}

/// Existing Fabric Pi usage schema 1 only. No generic SQLite/gateway inference.
/// These records identify sessions, not branches, native entries or work episodes.
pub(crate) fn usage(path: Option<&Path>, session: &Session) -> Result<UsageEvidence> {
    let mut report = UsageEvidence {
        coverage: "not_selected",
        ..Default::default()
    };
    let Some(path) = path else { return Ok(report) };
    let started = Instant::now();
    let file = inspection::open_source(path, MAX_BYTES)?;
    let before = file.metadata()?;
    let mut reader = BufReader::new(file);
    report.selected = true;
    report.coverage = "partial_session_attribution_without_work_or_branch_correlation";
    report.files_read = 1;
    let mut line = Vec::new();
    loop {
        line.clear();
        let count = (&mut reader)
            .take(MAX_RECORD_BYTES + 1)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            break;
        }
        report.bytes_read += count as u64;
        report.records_read += 1;
        ensure!(
            report.bytes_read <= MAX_BYTES,
            "usage log exceeds byte limit"
        );
        ensure!(
            report.records_read <= MAX_RECORDS,
            "usage log exceeds record limit"
        );
        ensure!(
            line.len() as u64 <= MAX_RECORD_BYTES,
            "usage record exceeds byte limit"
        );
        ensure!(line.ends_with(b"\n"), "incomplete usage log write");
        let value: Value = serde_json::from_slice(&line).context("invalid usage log JSON")?;
        if value["schema_version"] != 1 {
            report.unsupported_records += 1;
            continue;
        }
        let Some(id) = value["attribution"]["session_id"]
            .as_str()
            .filter(|id| crate::identifier(id))
        else {
            report.unattributed_records += 1;
            continue;
        };
        if id != session.native_id {
            continue;
        }
        match value["attribution"]["source"].as_str() {
            Some("main") => {
                if value["cwd"].as_str() != session.cwd.to_str() {
                    report.identity_conflicts += 1;
                    continue;
                }
                report.matching_main_records += 1;
                match value["stop_reason"].as_str() {
                    Some("aborted") => report.main_cancellations += 1,
                    Some("error") => report.main_errors += 1,
                    Some("stop" | "length") => report.main_turn_boundaries += 1,
                    Some("toolUse") => {}
                    _ => report.unsupported_records += 1,
                }
            }
            Some("child") => {
                // Child usage is attributed to its parent for accounting. It does NOT
                // establish the child's own identity, ownership or parent's cancellation.
                report.matching_child_records += 1;
                report.child_cancellations += usize::from(value["stop_reason"] == "aborted");
            }
            _ => report.unsupported_records += 1,
        }
    }
    inspection::unchanged(path, reader.get_ref(), &before)?;
    report.elapsed_micros = started.elapsed().as_micros();
    Ok(report)
}

#[derive(Serialize)]
pub struct Gate {
    pub decision: &'static str,
    pub automatic_candidate: bool,
    pub scope: &'static str,
    pub signals: Vec<Reason>,
    pub unresolved: Vec<&'static str>,
    pub usage_log: UsageEvidence,
}

pub(crate) fn gate(
    lineage: &PersistedLineage,
    policy_allowed: bool,
    retained_blocker: bool,
    usage_log: UsageEvidence,
) -> Gate {
    let mut blocked = !policy_allowed || retained_blocker;
    let mut signals = Vec::new();
    let mut add = |code, source| signals.push(Reason { code, source });
    if !policy_allowed {
        add("host_or_session_policy_blocks_recovery", "reignite_policy");
    }
    if retained_blocker {
        add("retained_state_blocks_recovery", "reignite_retained_state");
    }
    if lineage.reported_work.aborted_messages > 0 {
        blocked = true;
        add("reported_work_cancelled", "raw_persisted_lineage");
    }
    if lineage.reported_work.error_messages > 0 {
        blocked = true;
        add("reported_work_error", "raw_persisted_lineage");
    }
    if lineage.last_message_role == Some("assistant")
        && matches!(
            lineage.reported_work.last_assistant_stop_reason,
            Some("stop" | "length")
        )
    {
        blocked = true;
        // Native model-message termination is NOT proof of task completion or no dialogs.
        add("reported_turn_ended_or_waiting", "raw_persisted_lineage");
    }
    if lineage.unmatched_tool_calls > 0
        || lineage.unmatched_tool_results > 0
        || lineage.ambiguous_tool_calls > 0
        || lineage.failed_or_unknown_tool_results > 0
    {
        add("tool_outcomes_uncertain", "raw_persisted_lineage");
    }
    if lineage.opaque_entries > 0 {
        add("context_projection_uncertain", "raw_persisted_lineage");
    }
    if usage_log.main_cancellations > 0 {
        add("unscoped_main_cancellation_reported", "pi_usage_log");
    }
    if usage_log.main_errors > 0 {
        add("unscoped_main_error_reported", "pi_usage_log");
    }
    if usage_log.main_turn_boundaries > 0 {
        add("unscoped_main_turn_boundary_reported", "pi_usage_log");
    }
    if usage_log.matching_child_records > 0 {
        add("child_accounting_not_execution_evidence", "pi_usage_log");
    }
    if usage_log.identity_conflicts > 0 {
        add("usage_identity_conflict", "pi_usage_log");
    }
    if usage_log.unattributed_records > 0 || usage_log.unsupported_records > 0 {
        add("usage_attribution_or_format_incomplete", "pi_usage_log");
    }
    if usage_log.matching_main_records == 0 {
        add("no_matching_main_usage_evidence", "pi_usage_log");
    }
    Gate {
        decision: if blocked { "blocked" } else { "hold" },
        automatic_candidate: false,
        scope: "reported_user_message_segment_not_authorized_recovery_episode",
        signals,
        unresolved: vec![
            "pre_persistence_cancellation_not_excluded",
            "authoritative_branch_and_human_decision_state_missing",
            "execution_owner_profile_and_recovery_authorization_missing",
            "tool_and_child_survival_not_established",
            "sources_not_an_atomic_crash_time_snapshot",
            "timestamps_do_not_prove_causal_shutdown_or_work_correlation",
        ],
        usage_log,
    }
}
