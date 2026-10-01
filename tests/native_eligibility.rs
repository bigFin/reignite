//! Native assessment at the CLI boundary. No harness launch or fabricated authority.
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    process::{Command, Stdio},
};
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
    session: PathBuf,
    cwd: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("native with spaces.jsonl");
        let cwd = fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
        let f = Self { root, session, cwd };
        f.history(vec![user("user1", Value::Null)]);
        f
    }
    fn history(&self, entries: Vec<Value>) {
        let mut text = format!(
            "{}\n",
            json!({"type":"session","version":3,"id":"native-id","cwd":self.cwd})
        );
        for entry in entries {
            text.push_str(&format!("{entry}\n"));
        }
        fs::write(&self.session, text).unwrap();
        fs::set_permissions(&self.session, fs::Permissions::from_mode(0o600)).unwrap();
    }
    fn state(&self) -> PathBuf {
        self.root.path().join("state/state.json")
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_reignite"));
        command
            .env_remove("PI_SUBAGENTS_TEMP_ROOT")
            .arg("--state-dir")
            .arg(self.root.path().join("state"));
        command
    }
    fn cli(&self, args: &[&str]) -> Value {
        let output = self
            .command()
            .args(args)
            .arg("--session")
            .arg(&self.session)
            .output()
            .unwrap();
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE-CANARY"));
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output.status.success(), value["ok"] == true);
        value
    }
    fn assess(&self) -> Value {
        let source = fs::read(&self.session).unwrap();
        let state = fs::read(self.state()).ok();
        let value = self.cli(&["assess-pi"]);
        assert_eq!(fs::read(&self.session).unwrap(), source);
        assert_eq!(fs::read(self.state()).ok(), state);
        assert_eq!(value["ok"], true, "{value}");
        let report = value["result"].clone();
        assert_eq!(report["eligible"], false);
        assert_eq!(report["decision"], "observe_only");
        for (action, allowed) in report["actions"].as_object().unwrap() {
            assert_eq!(allowed, &(action == "inspect"), "{report}");
        }
        for fact in [
            "authoritative_pre_crash_branch",
            "interrupted_work_intent_and_cancellation",
            "human_decision_state",
            "exclusive_execution_owner",
            "original_resource_permission_profile",
            "authorized_native_recovery_episode",
            "complete_tool_and_child_survival_evidence",
        ] {
            assert!(
                report["requirements"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item["fact"] == fact && item["state"] == "missing"),
                "{report}"
            );
        }
        report
    }
    fn request(&self, request: Value) -> Value {
        let mut child = self
            .command()
            .arg("request")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(request.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(output.status.success(), "{value}");
        value["result"].clone()
    }
    fn register_busy(&self) {
        let identity = json!({"harness":"pi","session_id":"native-id","session_file":self.session,"cwd":self.cwd,"leaf":"user1"});
        for op in ["open", "enable", "observe"] {
            let mut request = json!({"op":op,"identity":identity,"pid":std::process::id()});
            if op == "open" {
                request["ticket"] = Value::Null;
            }
            if op == "observe" {
                request["activity"] = json!("busy");
            }
            self.request(request);
        }
    }
}
fn user(id: &str, parent: Value) -> Value {
    json!({"type":"message","id":id,"parentId":parent,"message":{"role":"user","content":"PRIVATE-CANARY"}})
}
fn assistant(id: &str, parent: &str, stop: &str, content: Value) -> Value {
    json!({"type":"message","id":id,"parentId":parent,"message":{"role":"assistant","stopReason":stop,"content":content}})
}
fn has(report: &Value, code: &str) -> bool {
    report["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["code"] == code)
}

#[test]
fn host_default_and_user_history_never_supply_missing_native_authority() {
    let f = Fixture::new();
    let report = f.assess();
    assert_eq!(report["policy_allowed"], true);
    assert_eq!(report["persisted_lineage"]["last_message_role"], "user");
    assert!(!f.state().exists());
    let recover = f.cli(&["recover", "--dry-run"]);
    assert_eq!(recover["result"]["eligible"], false);
    assert_eq!(f.cli(&["recover"])["ok"], false);
    assert!(!f.state().exists());
}
#[test]
fn assistant_questions_and_finished_work_are_held_through_metadata() {
    for stop in ["stop", "length", "aborted", "error"] {
        let f = Fixture::new();
        f.history(vec![
            user("user1", Value::Null),
            assistant(
                "answer",
                "user1",
                stop,
                json!([{"type":"text","text":"PRIVATE-CANARY Choose a branch"}]),
            ),
            json!({"type":"session_info","id":"name1","parentId":"answer","name":"PRIVATE-CANARY"}),
        ]);
        let report = f.assess();
        assert_eq!(
            report["session"]["last_persisted_message_role"],
            Value::Null
        );
        assert_eq!(
            report["persisted_lineage"]["last_message_role"],
            "assistant"
        );
        assert_eq!(
            report["persisted_lineage"]["last_assistant_stop_reason"],
            stop
        );
        assert!(has(
            &report,
            if matches!(stop, "aborted" | "error") {
                "reported_cancel_or_error"
            } else {
                "reported_assistant_turn_ended_or_waiting"
            }
        ));
    }
}
#[test]
fn raw_lineage_does_not_merge_abandoned_branches_or_require_a_single_root() {
    let f = Fixture::new();
    f.history(vec![
        user("user1", Value::Null),
        assistant("abandoned", "user1", "aborted", json!([])),
        user("other", json!("user1")),
    ]);
    let report = f.assess();
    assert_eq!(
        report["session"]["last_reported_assistant_stop_reason"],
        "aborted"
    );
    assert_eq!(
        report["persisted_lineage"]["last_assistant_stop_reason"],
        Value::Null
    );
    assert!(!has(&report, "reported_cancel_or_error"));
    assert_eq!(report["persisted_lineage"]["entries"], 2);
    f.history(vec![
        user("user1", Value::Null),
        user("another-root", Value::Null),
    ]);
    assert_eq!(f.assess()["persisted_lineage"]["entries"], 1);
}
#[test]
fn tool_pairing_does_not_establish_receipt_or_side_effect_safety() {
    for (second_result, expected_pending, expected_failed) in
        [(None, 1, 0), (Some(false), 0, 0), (Some(true), 0, 1)]
    {
        let f = Fixture::new();
        let mut entries = vec![
            user("user1", Value::Null),
            assistant(
                "calls",
                "user1",
                "toolUse",
                json!([
            {"type":"toolCall","id":"call-1","name":"PRIVATE-CANARY","arguments":{"secret":"PRIVATE-CANARY"}},
            {"type":"toolCall","id":"call-2","name":"bash","arguments":{}}]),
            ),
            json!({"type":"message","id":"result1","parentId":"calls","message":{"role":"toolResult","toolCallId":"call-1","isError":false,"content":"PRIVATE-CANARY"}}),
        ];
        if let Some(error) = second_result {
            entries.push(json!({"type":"message","id":"result2","parentId":"result1","message":{"role":"toolResult","toolCallId":"call-2","isError":error,"content":[]}}));
        }
        f.history(entries);
        let report = f.assess();
        assert_eq!(
            report["persisted_lineage"]["unmatched_tool_calls"],
            expected_pending
        );
        assert_eq!(
            report["persisted_lineage"]["failed_or_unknown_tool_results"],
            expected_failed
        );
        assert_eq!(
            has(&report, "reported_tool_outcome_uncertain"),
            expected_pending > 0 || expected_failed > 0
        );
    }
}
#[test]
fn summaries_context_edits_and_custom_authority_markers_are_opaque() {
    for kind in [
        "compaction",
        "branch_summary",
        "context_edit",
        "custom",
        "custom_message",
        "future-kind",
    ] {
        let f = Fixture::new();
        f.history(vec![user("user1", Value::Null), json!({"type":kind,"id":"marker","parentId":"user1",
            "customType":"recovery-eligible","data":{"eligible":true,"owner_verified":true,"approval":true},
            "summary":"PRIVATE-CANARY","targetId":"user1","replacement":null})]);
        let report = f.assess();
        assert_eq!(report["persisted_lineage"]["opaque_entries"], 1);
        assert!(has(&report, "opaque_or_transformed_persisted_context"));
    }
    let f = Fixture::new();
    f.history(vec![json!({"type":"message","id":"system1","parentId":null,"message":{"role":"system","sections":{"preamble":"PRIVATE-CANARY"},"toolsAdded":[{"name":"PRIVATE-CANARY","parameters":{"secret":"PRIVATE-CANARY"}}]}}), user("user1", json!("system1"))]);
    assert_eq!(
        f.assess()["persisted_lineage"]["system_loadout_reported"],
        true
    );
}
#[test]
fn damaged_lineage_incomplete_writes_and_limits_fail_without_state_mutation() {
    let f = Fixture::new();
    for entries in [
        vec![user("same", Value::Null), user("same", Value::Null)],
        vec![user("child", json!("missing"))],
        vec![user("child", json!("future")), user("future", Value::Null)],
        vec![user("self", json!("self"))],
        vec![
            json!({"type":"message","id":"no-parent","message":{"role":"user","content":"PRIVATE-CANARY"}}),
        ],
    ] {
        f.history(entries);
        assert_eq!(f.cli(&["assess-pi"])["ok"], false);
        assert!(!f.state().exists());
    }
    f.history(vec![user("user1", Value::Null)]);
    let mut bytes = fs::read(&f.session).unwrap();
    bytes.pop();
    fs::write(&f.session, &bytes).unwrap();
    assert_eq!(f.cli(&["assess-pi"])["ok"], false);
    assert_eq!(fs::read(&f.session).unwrap(), bytes);
    f.history(
        (0..65_537)
            .map(|i| json!({"type":"session_info","id":format!("entry{i}"),"parentId":null}))
            .collect(),
    );
    assert_eq!(f.cli(&["assess-pi"])["ok"], false);
    assert!(!f.state().exists());
}
#[test]
fn native_disable_follows_aliases_and_policy_toggles_never_authorize() {
    let mut f = Fixture::new();
    f.cli(&["disable"]);
    let alias = f.root.path().join("copy.jsonl");
    fs::copy(&f.session, &alias).unwrap();
    f.session = alias;
    assert!(has(&f.assess(), "native_session_disabled"));
    f.request(json!({"op":"set_host_policy","enabled":false}));
    assert!(has(&f.assess(), "host_recovery_disabled"));
    f.request(json!({"op":"set_host_policy","enabled":true}));
    assert_eq!(f.assess()["policy_allowed"], false);
    f.request(json!({"op":"clear_disable","session_file":f.session}));
    assert_eq!(f.assess()["policy_allowed"], true);
}
#[test]
fn all_retained_attempt_states_survive_assessment_aliases_and_schema_one_reads() {
    for status in ["authorized", "claimed", "accepted"] {
        let mut f = Fixture::new();
        f.register_busy();
        let mut db: Value = serde_json::from_slice(&fs::read(f.state()).unwrap()).unwrap();
        let record = db["records"]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .next()
            .unwrap();
        record["owner"]["boot"] = json!("00000000-0000-4000-8000-000000000001");
        record["attempt"] = json!({"id":"00000000-0000-4000-8000-000000000002","boot":"00000000-0000-4000-8000-000000000001","expires":0,"status":status});
        db["schema"] = json!(1);
        db.as_object_mut().unwrap().remove("policy");
        fs::write(f.state(), db.to_string()).unwrap();
        let old = f.session.clone();
        let alias = f.root.path().join("copy.jsonl");
        fs::copy(&old, &alias).unwrap();
        f.session = alias;
        let report = f.assess();
        assert_eq!(report["migration_pending"], true);
        assert_eq!(
            report["retained_records"][0]["session_file"],
            old.to_str().unwrap()
        );
        assert_eq!(report["retained_records"][0]["attempt"]["status"], status);
        assert!(has(
            &report,
            match status {
                "authorized" => "retained_authorized_attempt",
                "claimed" => "retained_uncertain_delivery",
                _ => "retained_api_acceptance_not_completion",
            }
        ));
    }
}
#[test]
fn missing_workspace_and_child_storage_do_not_require_loading_or_repair() {
    let mut f = Fixture::new();
    f.cwd = f.root.path().join("removed-worktree");
    f.history(vec![user("user1", Value::Null)]);
    assert!(has(&f.assess(), "workspace_unavailable_or_noncanonical"));
    let runtime = f.root.path().join("runtime");
    let run = runtime.join("async-subagent-runs/child1");
    fs::create_dir_all(&run).unwrap();
    let status = run.join("status.json");
    fs::write(&status, json!({"runId":"child1","sessionId":"native-id","state":"running","prompt":"PRIVATE-CANARY"}).to_string()).unwrap();
    fs::set_permissions(&status, fs::Permissions::from_mode(0o600)).unwrap();
    let before = fs::read(&status).unwrap();
    let output = f
        .command()
        .args(["assess-pi", "--session"])
        .arg(&f.session)
        .arg("--subagents-root")
        .arg(runtime)
        .output()
        .unwrap();
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(output.status.success(), "{response}");
    assert_eq!(response["result"]["delegated"]["coverage"], "partial");
    assert_eq!(
        response["result"]["delegated"]["runs"][0]["attribution_verified"],
        false
    );
    assert_eq!(fs::read(status).unwrap(), before);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE-CANARY"));
}
#[test]
fn legacy_busy_and_a_known_local_pid_are_not_native_intent_or_exclusivity() {
    let f = Fixture::new();
    f.register_busy();
    let report = f.assess();
    assert_eq!(report["retained_records"][0]["activity"], "busy");
    assert_eq!(
        report["retained_records"][0]["local_owner_reported_live"],
        true
    );
    assert!(has(&report, "retained_local_owner_live"));
    let dry_run = f.cli(&["recover", "--dry-run"]);
    assert_eq!(dry_run["result"]["eligibility_scope"], "legacy_prototype");
    assert_eq!(dry_run["result"]["native_eligibility_verified"], false);
}

#[test]
fn unsafe_sources_and_corrupt_policy_never_become_default_allow() {
    let mut f = Fixture::new();
    let original = f.session.clone();
    let alias = f.root.path().join("linked.jsonl");
    symlink(&original, &alias).unwrap();
    f.session = alias;
    assert_eq!(f.cli(&["assess-pi"])["ok"], false);
    f.session = original;
    fs::set_permissions(&f.session, fs::Permissions::from_mode(0o660)).unwrap();
    assert_eq!(f.cli(&["assess-pi"])["ok"], false);
    fs::set_permissions(&f.session, fs::Permissions::from_mode(0o600)).unwrap();
    f.request(json!({"op":"set_host_policy","enabled":true}));
    fs::write(f.state(), "{corrupt").unwrap();
    assert_eq!(f.cli(&["assess-pi"])["ok"], false);
    assert_eq!(fs::read_to_string(f.state()).unwrap(), "{corrupt");
}
