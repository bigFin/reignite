//! Policy-aware native Pi assessment. No enrollment, loading or authorization.
pub use crate::pi_evidence::PersistedLineage;
use crate::{
    Activity, Attempt, Record,
    inspection::{Delegated, Inspection, Session},
    policy::Policy,
};
use serde::Serialize;
use std::{fs, path::PathBuf};

#[derive(Serialize)]
pub struct Assessment {
    pub schema: u32,
    pub harness: &'static str,
    pub session: Session,
    pub eligible: bool,
    pub decision: &'static str,
    pub actions: Actions,
    pub policy_allowed: bool,
    pub requirements: Vec<Requirement>,
    pub reasons: Vec<Reason>,
    pub persisted_lineage: PersistedLineage,
    pub delegated: Delegated,
    pub retained_records: Vec<Retained>,
    pub state_schema: u32,
    pub migration_pending: bool,
    pub native_files_read: usize,
    pub native_bytes_read: u64,
    pub inspection_elapsed_micros: u128,
}
#[derive(Serialize)]
pub struct Actions {
    pub inspect: bool,
    pub acquire_execution_owner: bool,
    pub submit_continuation: bool,
    pub answer_human_decision: bool,
    pub resume_child: bool,
}
#[derive(Serialize)]
pub struct Requirement {
    pub fact: &'static str,
    pub state: &'static str,
    pub source: &'static str,
}
#[derive(Serialize)]
pub struct Reason {
    pub code: &'static str,
    pub source: &'static str,
}
#[derive(Serialize)]
pub struct Retained {
    pub session_file: PathBuf,
    pub enabled: bool,
    pub activity: Activity,
    pub shutdown_ambiguous: bool,
    /// Known local retained owner only; never exclusive conversation ownership.
    pub local_owner_reported_live: bool,
    pub attempt: Option<Attempt>,
}
fn reason(reasons: &mut Vec<Reason>, code: &'static str, source: &'static str) {
    if !reasons
        .iter()
        .any(|reason| reason.code == code && reason.source == source)
    {
        reasons.push(Reason { code, source });
    }
}

pub(crate) fn assess<'a>(
    inspection: Inspection,
    lineage: PersistedLineage,
    policy: &Policy,
    records: impl Iterator<Item = &'a Record>,
    boot: &str,
    state_schema: u32,
) -> Assessment {
    let identity = crate::Identity {
        harness: "pi".into(),
        session_id: inspection.session.native_id.clone(),
        session_file: inspection.session.file.clone(),
        cwd: inspection.session.cwd.clone(),
        leaf: None,
    };
    let mut reasons = Vec::new();
    if !policy.host_enabled {
        reason(&mut reasons, "host_recovery_disabled", "reignite_policy");
    }
    if policy.disabled(&identity).is_some() {
        reason(&mut reasons, "native_session_disabled", "reignite_policy");
    }
    let policy_allowed = policy.blocked_reason(&identity).is_none();
    let workspace_available =
        fs::canonicalize(&identity.cwd).is_ok_and(|path| path == identity.cwd && path.is_dir());
    if !workspace_available {
        reason(
            &mut reasons,
            "workspace_unavailable_or_noncanonical",
            "current_filesystem",
        );
    }
    let mut retained_records = Vec::new();
    let mut retained_blocker = false;
    for record in records.filter(|record| {
        record.identity.harness == identity.harness
            && record.identity.session_id == identity.session_id
    }) {
        if !record.enabled {
            reason(
                &mut reasons,
                "retained_session_disabled",
                "reignite_retained_state",
            );
            retained_blocker = true;
        }
        match record.activity {
            Activity::Stopped => {
                reason(
                    &mut reasons,
                    "retained_session_stopped",
                    "reignite_retained_state",
                );
                retained_blocker = true;
            }
            Activity::Waiting => {
                reason(
                    &mut reasons,
                    "retained_session_waiting",
                    "reignite_retained_state",
                );
                retained_blocker = true;
            }
            Activity::Idle => {
                reason(
                    &mut reasons,
                    "retained_session_idle",
                    "reignite_retained_state",
                );
                retained_blocker = true;
            }
            Activity::Busy => {} // Busy is not native crash-time intent or human-wait evidence.
        }
        if record.shutdown_ambiguous {
            reason(
                &mut reasons,
                "retained_shutdown_ambiguous",
                "reignite_retained_state",
            );
            retained_blocker = true;
        }
        let owner_live = crate::live(&record.owner, boot);
        if owner_live {
            reason(
                &mut reasons,
                "retained_local_owner_live",
                "reignite_retained_state_and_current_kernel",
            );
            retained_blocker = true;
        }
        if let Some(attempt) = &record.attempt {
            reason(
                &mut reasons,
                match attempt.status.as_str() {
                    "authorized" => "retained_authorized_attempt",
                    "claimed" => "retained_uncertain_delivery",
                    "accepted" => "retained_api_acceptance_not_completion",
                    _ => unreachable!("store validated attempt"),
                },
                "reignite_retained_state",
            );
            // Expiry, another boot, copied files or host toggles never rearm an attempt.
            retained_blocker = true;
        }
        retained_records.push(Retained {
            session_file: record.identity.session_file.clone(),
            enabled: record.enabled,
            activity: record.activity,
            shutdown_ambiguous: record.shutdown_ambiguous,
            local_owner_reported_live: owner_live,
            attempt: record.attempt.clone(),
        });
    }
    if matches!(
        lineage.last_assistant_stop_reason,
        Some("aborted" | "error")
    ) {
        reason(
            &mut reasons,
            "reported_cancel_or_error",
            "raw_persisted_lineage",
        );
    }
    match (
        lineage.last_message_role,
        lineage.last_assistant_stop_reason,
    ) {
        (Some("assistant"), Some("stop" | "length")) => reason(
            &mut reasons,
            "reported_assistant_turn_ended_or_waiting",
            "raw_persisted_lineage",
        ),
        (None, _) => reason(
            &mut reasons,
            "no_reported_conversation_message",
            "raw_persisted_lineage",
        ),
        _ => {}
    }
    if lineage.unmatched_tool_calls > 0
        || lineage.unmatched_tool_results > 0
        || lineage.ambiguous_tool_calls > 0
        || lineage.failed_or_unknown_tool_results > 0
    {
        reason(
            &mut reasons,
            "reported_tool_outcome_uncertain",
            "raw_persisted_lineage",
        );
    }
    if lineage.opaque_entries > 0 {
        reason(
            &mut reasons,
            "opaque_or_transformed_persisted_context",
            "raw_persisted_lineage",
        );
    }
    let mut requirements = vec![
        Requirement {
            fact: "host_session_policy",
            state: if policy_allowed {
                "verified"
            } else {
                "blocked"
            },
            source: "reignite_policy",
        },
        Requirement {
            fact: "selected_native_file_identity",
            state: "verified",
            source: "bounded_same_user_native_header",
        },
        Requirement {
            fact: "workspace_available",
            state: if workspace_available {
                "verified"
            } else {
                "blocked"
            },
            source: "current_filesystem",
        },
        Requirement {
            fact: "retained_attempt_and_blocker_safety",
            state: if retained_blocker {
                "blocked"
            } else {
                "verified"
            },
            source: "reignite_retained_state",
        },
    ];
    for fact in [
        "authoritative_pre_crash_branch",
        "interrupted_work_intent_and_cancellation",
        "human_decision_state",
        "exclusive_execution_owner",
        "original_resource_permission_profile",
        "authorized_native_recovery_episode",
        "complete_tool_and_child_survival_evidence",
    ] {
        requirements.push(Requirement {
            fact,
            state: "missing",
            source: "not_established_by_native_history_or_retained_state",
        });
        reason(&mut reasons, fact, "missing_authoritative_evidence");
    }
    Assessment {
        schema: 1,
        harness: "pi",
        session: inspection.session,
        eligible: false,
        decision: "observe_only",
        actions: Actions {
            inspect: true,
            acquire_execution_owner: false,
            submit_continuation: false,
            answer_human_decision: false,
            resume_child: false,
        },
        policy_allowed,
        requirements,
        reasons,
        persisted_lineage: lineage,
        delegated: inspection.delegated,
        retained_records,
        state_schema,
        migration_pending: state_schema == 1,
        native_files_read: inspection.files_read,
        native_bytes_read: inspection.bytes_read,
        inspection_elapsed_micros: inspection.elapsed_micros,
    }
}
