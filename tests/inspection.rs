//! Standalone native-storage CLI tests: no extension, provider, or live state.
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;

struct Fixture {
    temp: TempDir,
    session: PathBuf,
    runtime: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let session = temp.path().join("parent with spaces.jsonl");
        let runtime = temp.path().join("runtime with spaces");
        fs::create_dir(&runtime).unwrap();
        let fixture = Self {
            temp,
            session,
            runtime,
        };
        fixture.history(3, "toolUse");
        fixture
    }
    fn history(&self, version: u32, reason: &str) {
        let entries = [
            json!({"type":"session", "version":version, "id":"parent-id", "cwd":self.temp.path()}),
            json!({"type":"message", "id":"user1", "parentId":null, "message":{"role":"user", "content":"PRIVATE-USER-CANARY"}}),
            json!({"type":"message", "id":"assistant1", "parentId":"user1", "message":{"role":"assistant", "stopReason":reason, "content":[{"type":"toolCall", "id":"call1", "name":"bash", "arguments":{"command":"PRIVATE-COMMAND-CANARY"}}]}}),
        ];
        let text = entries
            .iter()
            .map(|entry| format!("{entry}\n"))
            .collect::<String>();
        private_write(&self.session, &text);
    }
    fn run(&self, children: bool) -> Value {
        let mut command = Command::new(env!("CARGO_BIN_EXE_reignite"));
        command
            .env_remove("PI_SUBAGENTS_TEMP_ROOT")
            .arg("--state-dir")
            .arg(self.temp.path().join("must-not-be-created"))
            .arg("inspect")
            .arg("--session")
            .arg(&self.session);
        if children {
            command.arg("--subagents-root").arg(&self.runtime);
        }
        let output = command.output().unwrap();
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output.status.success(), response["ok"] == true);
        assert!(!self.temp.path().join("must-not-be-created").exists());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE-"));
        response
    }
    fn child(&self, id: &str, parent: &str, state: &str) -> PathBuf {
        let dir = self.runtime.join("async-subagent-runs").join(id);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("status.json");
        private_write(
            &path,
            &json!({"runId":id,"sessionId":parent,"state":state,"pid":std::process::id(),"prompt":"PRIVATE-CHILD-CANARY"}).to_string(),
        );
        path
    }
}
fn private_write(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn native_history_needs_no_enrollment_and_creates_no_recovery_state() {
    let fixture = Fixture::new();
    let before = fs::read(&fixture.session).unwrap();
    let response = fixture.run(false);
    assert_eq!(response["ok"], true, "{response}");
    let report = &response["result"];
    assert_eq!(report["session"]["native_id"], "parent-id");
    assert_eq!(report["session"]["last_persisted_entry"], "assistant1");
    assert_eq!(report["session"]["entries"], 2);
    assert_eq!(report["eligible"], false);
    assert!(report["missing_evidence"].as_array().unwrap().len() >= 5);
    assert_eq!(report["delegated"]["coverage"], "unavailable");
    assert_eq!(report["files_read"], 1);
    assert_eq!(report["bytes_read"], before.len() as u64);
    assert_eq!(fs::read(&fixture.session).unwrap(), before);
}

#[test]
fn reports_only_matching_async_ids_without_repairing_or_verifying_attribution() {
    let fixture = Fixture::new();
    let matching = fixture.child("run1", "parent-id", "running");
    fixture.child("other-run", "other-parent", "complete");
    let before = fs::read(&matching).unwrap();
    let response = fixture.run(true);
    assert_eq!(response["ok"], true, "{response}");
    let report = &response["result"];
    assert_eq!(report["delegated"]["coverage"], "partial");
    assert_eq!(report["delegated"]["runs"].as_array().unwrap().len(), 1);
    assert_eq!(report["delegated"]["runs"][0]["run_id"], "run1");
    assert_eq!(report["delegated"]["runs"][0]["reported_state"], "running");
    assert_eq!(
        report["delegated"]["runs"][0]["attribution_verified"],
        false
    );
    assert_eq!(report["files_read"], 3);
    assert_eq!(fs::read(&matching).unwrap(), before);
    assert_eq!(fs::read_dir(matching.parent().unwrap()).unwrap().count(), 1);
}

#[test]
fn persisted_parent_references_match_exact_file_paths_not_basenames_or_aliases() {
    let fixture = Fixture::new();
    let parent = fixture.session.to_str().unwrap();
    fixture.child("exact-file", parent, "running");
    fixture.child("native-fallback", "parent-id", "paused");
    let other = fixture
        .temp
        .path()
        .join("other")
        .join("parent with spaces.jsonl");
    fixture.child("same-basename", other.to_str().unwrap(), "running");
    let alias = fixture.temp.path().join("alias.jsonl");
    symlink(&fixture.session, &alias).unwrap();
    fixture.child("aliased-parent", alias.to_str().unwrap(), "running");
    let response = fixture.run(true);
    let runs = response["result"]["delegated"]["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2);
    let exact = runs
        .iter()
        .find(|run| run["run_id"] == "exact-file")
        .unwrap();
    assert_eq!(exact["reported_parent_session_ref"], parent);
    assert_eq!(exact["attribution_verified"], false);
    assert_eq!(response["result"]["eligible"], false);
}

#[test]
fn absent_and_empty_child_storage_are_not_complete_coverage() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.run(true)["result"]["delegated"]["coverage"],
        "unavailable"
    );
    fs::create_dir(fixture.runtime.join("async-subagent-runs")).unwrap();
    let response = fixture.run(true);
    assert_eq!(response["result"]["delegated"]["coverage"], "partial");
    assert_eq!(response["result"]["eligible"], false);
}

#[test]
fn corrupt_unknown_and_mismatched_run_records_never_become_resume_targets() {
    let fixture = Fixture::new();
    let corrupt = fixture.child("corrupt", "parent-id", "running");
    private_write(&corrupt, "{broken");
    let mismatch = fixture.child("wrong-dir", "parent-id", "running");
    private_write(
        &mismatch,
        &json!({"runId":"different", "sessionId":"parent-id", "state":"running"}).to_string(),
    );
    fixture.child("unknown-state", "parent-id", "PRIVATE-STATE-CANARY");
    let before = fs::read(&corrupt).unwrap();
    let response = fixture.run(true);
    let delegated = &response["result"]["delegated"];
    assert_eq!(delegated["warnings"].as_array().unwrap().len(), 2);
    assert_eq!(delegated["runs"].as_array().unwrap().len(), 1);
    assert_eq!(delegated["runs"][0]["reported_state"], "unknown");
    assert_eq!(fs::read(&corrupt).unwrap(), before);
}

#[test]
fn cancellation_and_reported_completion_do_not_supply_missing_lifecycle_evidence() {
    for reason in ["aborted", "stop", "error", "toolUse"] {
        let fixture = Fixture::new();
        fixture.history(3, reason);
        let response = fixture.run(false);
        assert_eq!(
            response["result"]["session"]["last_reported_assistant_stop_reason"],
            reason
        );
        assert_eq!(response["result"]["eligible"], false);
    }
}

#[test]
fn unknown_format_and_incomplete_writes_are_rejected_without_repair() {
    let fixture = Fixture::new();
    fixture.history(99, "stop");
    assert_eq!(fixture.run(false)["ok"], false);
    fixture.history(3, "stop");
    let mut content = fs::read(&fixture.session).unwrap();
    content.pop();
    fs::write(&fixture.session, &content).unwrap();
    assert_eq!(fixture.run(false)["ok"], false);
    assert_eq!(fs::read(&fixture.session).unwrap(), content);
}

#[test]
fn source_symlinks_and_group_writable_sessions_are_rejected() {
    let mut fixture = Fixture::new();
    let link = fixture.temp.path().join("linked-session.jsonl");
    symlink(&fixture.session, &link).unwrap();
    let original = fixture.session.clone();
    fixture.session = link;
    assert_eq!(fixture.run(false)["ok"], false);
    fixture.session = original;
    fs::set_permissions(&fixture.session, fs::Permissions::from_mode(0o660)).unwrap();
    assert_eq!(fixture.run(false)["ok"], false);
}

#[test]
fn run_directory_and_status_symlinks_are_not_followed() {
    let fixture = Fixture::new();
    let original = fixture.child("original", "parent-id", "running");
    let runs = original.parent().unwrap().parent().unwrap();
    symlink(original.parent().unwrap(), runs.join("linked-dir")).unwrap();
    let dir = runs.join("linked-status");
    fs::create_dir(&dir).unwrap();
    symlink(&original, dir.join("status.json")).unwrap();
    let response = fixture.run(true);
    assert_eq!(
        response["result"]["delegated"]["warnings"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        response["result"]["delegated"]["runs"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn discovery_is_bounded_and_explicitly_truncated() {
    let fixture = Fixture::new();
    for index in 0..257 {
        fixture.child(&format!("run-{index}"), "parent-id", "running");
    }
    let response = fixture.run(true);
    assert_eq!(response["result"]["delegated"]["truncated"], true);
    assert_eq!(
        response["result"]["delegated"]["runs"]
            .as_array()
            .unwrap()
            .len(),
        256
    );
    assert_eq!(response["result"]["eligible"], false);
}

#[test]
fn oversized_sources_fail_before_unbounded_reads() {
    let fixture = Fixture::new();
    fs::OpenOptions::new()
        .write(true)
        .open(&fixture.session)
        .unwrap()
        .set_len(128 * 1024 * 1024 + 1)
        .unwrap();
    assert_eq!(fixture.run(false)["ok"], false);
}
