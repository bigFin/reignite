//! Transitional Pi RPC delivery using an existing one-use legacy ticket.
//! This proves transport, not extension-free native automatic eligibility.
use crate::{CONTINUATION, Identity, Request, Store, inspection, pi_rpc::Client};
use anyhow::{Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    os::unix::fs::MetadataExt,
    path::Path,
    time::{Duration, Instant},
};

#[derive(Serialize)]
pub struct Report {
    pub schema: u32,
    pub harness: &'static str,
    pub authorization_source: &'static str,
    pub native_eligibility_verified: bool,
    pub exclusive_conversation_owner_verified: bool,
    pub outcome: &'static str,
    pub reason: &'static str,
    pub api_acceptance_observed: bool,
    pub agent_settled_observed: bool,
    pub work_complete_verified: bool,
    pub dialog_methods: Vec<String>,
    pub stdout_bytes: usize,
    pub events: usize,
    pub elapsed_micros: u128,
}
fn report(
    client: Option<&Client>,
    started: Instant,
    outcome: &'static str,
    reason: &'static str,
    accepted: bool,
) -> Report {
    Report {
        schema: 1,
        harness: "pi",
        authorization_source: "legacy_restore_ticket",
        native_eligibility_verified: false,
        exclusive_conversation_owner_verified: false,
        outcome,
        reason,
        api_acceptance_observed: accepted,
        agent_settled_observed: client.is_some_and(|client| client.settled),
        work_complete_verified: false,
        dialog_methods: client
            .map(|client| client.dialogs.iter().cloned().collect())
            .unwrap_or_default(),
        stdout_bytes: client.map_or(0, |client| client.stdout_bytes),
        events: client.map_or(0, |client| client.events),
        elapsed_micros: started.elapsed().as_micros(),
    }
}
fn verify_state(state: &Value, identity: &Identity) -> Result<()> {
    ensure!(
        state["sessionId"].as_str() == Some(&identity.session_id)
            && state["sessionFile"].as_str() == identity.session_file.to_str(),
        "Pi runtime identity mismatch"
    );
    ensure!(
        state["isStreaming"].as_bool() == Some(false)
            && state["isCompacting"].as_bool() == Some(false)
            && state["pendingMessageCount"].as_u64() == Some(0),
        "Pi runtime busy or state unsupported"
    );
    Ok(())
}
fn history_hold(messages: &Value) -> Option<&'static str> {
    let Some(messages) = messages["messages"].as_array() else {
        return Some("unsupported_history");
    };
    if !matches!(
        messages.last().and_then(|message| message["role"].as_str()),
        Some("user" | "toolResult")
    ) {
        // Completed assistant turns may be questions or finished work. Never guess.
        return Some("waiting_or_uncertain_history");
    }
    let mut pending = BTreeSet::new();
    for message in messages {
        match message["role"].as_str() {
            Some("assistant") => {
                if let Some(content) = message["content"].as_array() {
                    for block in content {
                        if block["type"] == "toolCall" {
                            let Some(id) = block["id"].as_str() else {
                                return Some("unsupported_tool_history");
                            };
                            if id.len() > 256 || !pending.insert(id.to_owned()) {
                                return Some("ambiguous_tool_history");
                            }
                        }
                    }
                }
            }
            Some("toolResult") => {
                let Some(id) = message["toolCallId"].as_str() else {
                    return Some("unsupported_tool_history");
                };
                if !pending.remove(id) || message["isError"].as_bool() != Some(false) {
                    return Some("uncertain_tool_outcome");
                }
            }
            _ => {}
        }
    }
    if pending.is_empty() {
        None
    } else {
        Some("uncertain_tool_outcome")
    }
}

fn signature(path: &Path) -> Result<(u64, u64, u64, i64, i64, i64, i64)> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(meta.is_file(), "restore source is not a regular file");
    Ok((
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.ctime(),
        meta.ctime_nsec(),
    ))
}

pub fn deliver(
    store: &Store,
    session: &Path,
    ticket: &str,
    program: &Path,
    profile: &[OsString],
    timeout: Duration,
) -> Result<Report> {
    let started = Instant::now();
    crate::pi_rpc::validate_profile(program, profile)?;
    let record = store.pi_ticket_scope(session, ticket)?;
    let original = signature(session)?;
    let source = inspection::inspect(session, None)?.session;
    ensure!(
        signature(session)? == original,
        "restore source changed during inspection"
    );
    ensure!(
        source.native_id == record.identity.session_id
            && source.cwd == record.identity.cwd
            && (source.device, source.inode) == (record.device, record.inode),
        "restore source identity mismatch"
    );
    if source.last_persisted_entry != record.identity.leaf || record.identity.leaf.is_none() {
        return Ok(report(
            None,
            started,
            "held",
            "persisted_branch_changed_or_unknown",
            false,
        ));
    }
    if !matches!(
        source.last_persisted_message_role.as_deref(),
        Some("user" | "toolResult")
    ) {
        return Ok(report(
            None,
            started,
            "held",
            "waiting_or_uncertain_history",
            false,
        ));
    }
    if matches!(
        source.last_reported_assistant_stop_reason.as_deref(),
        Some("error" | "aborted")
    ) {
        return Ok(report(
            None,
            started,
            "held",
            "persisted_cancel_or_failure",
            false,
        ));
    }
    let mut client = Client::start(program, profile, session, &record.identity.cwd, timeout)?;
    let Some(state) = client.query("get_state", json!({}))? else {
        return Ok(report(
            Some(&client),
            started,
            "held",
            "startup_decision_or_extension_failure",
            false,
        ));
    };
    verify_state(&state, &record.identity)?;
    let Some(entries) = client.query("get_entries", json!({"since":record.identity.leaf}))? else {
        return Ok(report(
            Some(&client),
            started,
            "held",
            "startup_decision_or_extension_failure",
            false,
        ));
    };
    ensure!(
        entries["leafId"].as_str() == record.identity.leaf.as_deref()
            && entries["entries"]
                .as_array()
                .is_some_and(|entries| entries.is_empty()),
        "Pi runtime branch changed"
    );
    let Some(messages) = client.query("get_messages", json!({}))? else {
        return Ok(report(
            Some(&client),
            started,
            "held",
            "startup_decision_or_extension_failure",
            false,
        ));
    };
    if let Some(reason) = history_hold(&messages) {
        return Ok(report(Some(&client), started, "held", reason, false));
    }
    if client.activity_seen || !client.dialogs.is_empty() || client.failure_seen {
        return Ok(report(
            Some(&client),
            started,
            "held",
            "unexpected_startup_activity_or_decision",
            false,
        ));
    }
    if signature(session)? != original {
        return Ok(report(
            Some(&client),
            started,
            "held",
            "source_changed_during_load",
            false,
        ));
    }
    // Claim in the shared store BEFORE writing prompt bytes. Errors after here
    // remain consumed/uncertain. No retries, synthetic approvals or lifecycle feed.
    let claim = store.execute(Request::Open {
        identity: record.identity.clone(),
        pid: client.pid(),
        ticket: Some(ticket.to_owned()),
    })?;
    ensure!(
        claim["attempt"].as_str() == Some(ticket),
        "restore claim mismatch"
    );
    let policy = store.execute(Request::PolicyStatus {
        session_file: Some(session.to_owned()),
    })?;
    if policy["policy_enabled_for_session"].as_bool() != Some(true) {
        return Ok(report(
            Some(&client),
            started,
            "held",
            "policy_changed_after_claim",
            false,
        ));
    }
    if signature(session)? != original {
        return Ok(report(
            Some(&client),
            started,
            "held",
            "source_changed_after_claim",
            false,
        ));
    }
    client.send("prompt", json!({"message":CONTINUATION}))?;
    let Some(accepted) = client.response("prompt")? else {
        return Ok(report(
            Some(&client),
            started,
            "held",
            "decision_or_failure_after_submission_delivery_uncertain",
            false,
        ));
    };
    store.execute(Request::Accepted {
        identity: record.identity.clone(),
        pid: client.pid(),
        attempt: ticket.to_owned(),
    })?;
    match accepted["disposition"].as_str() {
        Some("started") => {}
        Some("handled" | "queued") => {
            return Ok(report(
                Some(&client),
                started,
                "held",
                "prompt_handled_or_queued_not_verified",
                true,
            ));
        }
        _ => {
            return Ok(report(
                Some(&client),
                started,
                "held",
                "unsupported_prompt_disposition",
                true,
            ));
        }
    }
    client.until_settled()?;
    if !client.dialogs.is_empty() || client.failure_seen {
        return Ok(report(
            Some(&client),
            started,
            "held",
            "human_decision_or_run_failure",
            true,
        ));
    }
    // Recheck identity/idle state: settlement is not proof of task completion.
    let Some(state) = client.query("get_state", json!({}))? else {
        return Ok(report(
            Some(&client),
            started,
            "held",
            "decision_after_settlement",
            true,
        ));
    };
    verify_state(&state, &record.identity)?;
    Ok(report(
        Some(&client),
        started,
        "settled",
        "agent_settled_not_work_completion",
        true,
    ))
}
