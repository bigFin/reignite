//! Native policy tests at the real CLI boundary. No harness launch or model calls.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
    session: PathBuf,
    identity: Value,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("native session.jsonl");
        let cwd = fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
        fs::write(&session, format!("{}\n{}\n", json!({"type":"session","version":3,"id":"native-session","cwd":cwd}),
            json!({"type":"message","id":"leaf1","parentId":null,"message":{"role":"user","content":"PRIVATE-POLICY-CANARY"}}))).unwrap();
        fs::set_permissions(&session, fs::Permissions::from_mode(0o600)).unwrap();
        let identity = json!({"harness":"pi","session_id":"native-session","session_file":session,"cwd":cwd,"leaf":"leaf1"});
        Self {
            root,
            session,
            identity,
        }
    }
    fn state(&self) -> PathBuf {
        self.root.path().join("state")
    }
    fn database(&self) -> PathBuf {
        self.state().join("state.json")
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_reignite"));
        command.arg("--state-dir").arg(self.state());
        command
    }
    fn cli(&self, args: &[&str], session: Option<&Path>) -> Value {
        let mut command = self.command();
        command.args(args);
        if let Some(session) = session {
            command.arg("--session").arg(session);
        }
        let output = command.output().unwrap();
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE-POLICY-CANARY"));
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output.status.success(), response["ok"] == true);
        response
    }
    fn ok(&self, args: &[&str], session: Option<&Path>) -> Value {
        let response = self.cli(args, session);
        assert_eq!(response["ok"], true, "{response}");
        response["result"].clone()
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
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn owner_request(&self, op: &str) -> Value {
        json!({"op":op,"identity":self.identity,"pid":std::process::id()})
    }
    fn busy(&self) {
        let mut open = self.owner_request("open");
        open["ticket"] = Value::Null;
        assert_eq!(self.request(open)["ok"], true);
        assert_eq!(self.request(self.owner_request("enable"))["ok"], true);
        let mut observe = self.owner_request("observe");
        observe["activity"] = json!("busy");
        assert_eq!(self.request(observe)["ok"], true);
    }
    fn read(&self) -> Value {
        serde_json::from_slice(&fs::read(self.database()).unwrap()).unwrap()
    }
    fn write(&self, value: &Value) {
        fs::write(self.database(), value.to_string()).unwrap();
    }
    fn legacy(&self, record_edit: impl FnOnce(&mut Value)) -> Value {
        self.busy();
        let mut db = self.read();
        record_edit(
            db["records"]
                .as_object_mut()
                .unwrap()
                .values_mut()
                .next()
                .unwrap(),
        );
        db["schema"] = json!(1);
        db.as_object_mut().unwrap().remove("policy");
        self.write(&db);
        db
    }
}
#[test]
fn host_default_needs_no_enrollment_and_does_not_authorize_work() {
    let f = Fixture::new();
    let before = fs::read(&f.session).unwrap();
    let report = f.ok(&["policy", "show"], Some(&f.session));
    assert_eq!(report["policy"]["host_enabled"], true);
    assert_eq!(report["policy_enabled_for_session"], true);
    assert_eq!(report["eligible"], false);
    assert_eq!(report["native_automatic_recovery_implemented"], false);
    assert!(!f.database().exists());
    let recovery = f.ok(&["recover", "--dry-run"], Some(&f.session));
    assert_eq!(recovery["eligible"], false);
    assert!(
        recovery["blocked_reason"]
            .as_str()
            .unwrap()
            .contains("not implemented")
    );
    assert_eq!(f.cli(&["recover"], Some(&f.session))["ok"], false);
    assert!(!f.database().exists());
    assert_eq!(fs::read(&f.session).unwrap(), before);
}
#[test]
fn unenrolled_disable_is_durable_follows_native_id_and_ignores_incomplete_tail() {
    let f = Fixture::new();
    // An active appending session or missing workspace must not prevent opting out.
    fs::write(&f.session, format!("{}\n{{partial", json!({"type":"session","version":3,"id":"native-session","cwd":f.root.path().join("removed-worktree")}))).unwrap();
    let before = fs::read(&f.session).unwrap();
    f.ok(&["disable"], Some(&f.session));
    let state = f.read();
    assert_eq!(state["schema"], 2);
    assert!(state["records"].as_object().unwrap().is_empty());
    assert_eq!(
        state["policy"]["disabled_sessions"]["pi:native-session"]["reason"],
        "explicit"
    );
    let moved = f.root.path().join("moved session.jsonl");
    fs::rename(&f.session, &moved).unwrap();
    assert_eq!(
        f.ok(&["policy", "show"], Some(&moved))["policy_enabled_for_session"],
        false
    );
    let recovery = f.ok(&["recover", "--dry-run"], Some(&moved));
    assert_eq!(
        recovery["blocked_reason"],
        "native session recovery disabled"
    );
    f.ok(&["policy", "disable"], None);
    f.ok(&["policy", "enable"], None);
    assert_eq!(
        f.ok(&["policy", "show"], Some(&moved))["policy_enabled_for_session"],
        false
    );
    let clear = f.ok(&["policy", "clear-disable"], Some(&moved));
    assert_eq!(clear["attempts_reset"], false);
    assert_eq!(
        f.ok(&["policy", "show"], Some(&moved))["policy_enabled_for_session"],
        true
    );
    assert_eq!(fs::read(&moved).unwrap(), before);
    assert!(f.read()["records"].as_object().unwrap().is_empty());
}
#[test]
fn host_disable_blocks_already_authorized_claim_and_enable_never_resets_attempts() {
    let f = Fixture::new();
    f.busy();
    let mut db = f.read();
    let record = db["records"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap();
    record["owner"]["boot"] = json!("00000000-0000-4000-8000-000000000001");
    f.write(&db);
    let ticket = f.ok(&["recover"], Some(&f.session))["ticket"].clone();
    let before = f.read()["records"].clone();
    f.ok(&["policy", "disable"], None);
    assert_eq!(
        f.ok(&["recover", "--dry-run"], Some(&f.session))["eligible"],
        false
    );
    let mut open = f.owner_request("open");
    open["ticket"] = ticket;
    assert_eq!(f.request(open)["ok"], false);
    assert_eq!(f.read()["records"], before);
    f.ok(&["policy", "enable"], None);
    assert_eq!(f.read()["records"], before);
    // Existing authorization/expiry remains intact; no new authorization is minted.
    assert_eq!(
        f.ok(&["recover", "--dry-run"], Some(&f.session))["eligible"],
        false
    );
}
#[test]
fn schema_one_migration_preserves_every_attempt_state_and_read_does_not_rewrite() {
    for status in ["authorized", "claimed", "accepted"] {
        let f = Fixture::new();
        let legacy = f.legacy(|record| {
            record["owner"]["boot"] = json!("00000000-0000-4000-8000-000000000001");
            record["attempt"] = json!({"id":"00000000-0000-4000-8000-000000000002","boot":"00000000-0000-4000-8000-000000000003","expires":123,"status":status});
            record["shutdown_ambiguous"] = json!(true);
        });
        let bytes = fs::read(f.database()).unwrap();
        let report = f.ok(&["policy", "show"], Some(&f.session));
        assert_eq!(report["migration_pending"], true);
        assert_eq!(report["legacy_attempts"][0]["status"], status);
        assert_eq!(fs::read(f.database()).unwrap(), bytes);
        f.ok(&["policy", "disable"], None);
        let migrated = f.read();
        assert_eq!(migrated["schema"], 2);
        assert_eq!(migrated["records"], legacy["records"]);
        f.ok(&["policy", "enable"], None);
        f.ok(&["policy", "clear-disable"], Some(&f.session));
        assert_eq!(f.read()["records"], legacy["records"]);
        assert_eq!(
            f.ok(&["recover", "--dry-run"], Some(&f.session))["eligible"],
            false
        );
    }
}
#[test]
fn migrated_disabled_and_stopped_records_stay_blocked_without_enrollment() {
    for stopped in [false, true] {
        let f = Fixture::new();
        let legacy = f.legacy(|record| {
            record["enabled"] = json!(stopped);
            record["activity"] = json!(if stopped { "stopped" } else { "busy" });
        });
        let report = f.ok(&["policy", "show"], Some(&f.session));
        assert_eq!(report["policy_enabled_for_session"], false);
        assert_eq!(
            report["policy"]["disabled_sessions"]["pi:native-session"]["reason"],
            "legacy_disabled_or_stopped"
        );
        f.ok(&["policy", "enable"], None);
        assert_eq!(f.read()["records"], legacy["records"]);
        f.ok(&["policy", "clear-disable"], Some(&f.session));
        assert_eq!(f.read()["records"], legacy["records"]);
        assert_eq!(
            f.ok(&["recover", "--dry-run"], Some(&f.session))["eligible"],
            false
        );
    }
}
#[test]
fn corrupt_or_incomplete_policy_schema_is_not_defaulted_or_replaced() {
    for case in [
        "missing_policy",
        "missing_host",
        "unknown_field",
        "wrong_key",
        "unknown_reason",
        "schema",
    ] {
        let f = Fixture::new();
        f.ok(&["disable"], Some(&f.session));
        let mut db = f.read();
        match case {
            "missing_policy" => {
                db.as_object_mut().unwrap().remove("policy");
            }
            "missing_host" => {
                db["policy"].as_object_mut().unwrap().remove("host_enabled");
            }
            "unknown_field" => db["policy"]["unrecognized"] = json!(true),
            "wrong_key" => {
                let entry = db["policy"]["disabled_sessions"]
                    .as_object_mut()
                    .unwrap()
                    .remove("pi:native-session")
                    .unwrap();
                db["policy"]["disabled_sessions"]["pi:other"] = entry;
            }
            "unknown_reason" => {
                db["policy"]["disabled_sessions"]["pi:native-session"]["reason"] = json!("maybe")
            }
            "schema" => db["schema"] = json!(3),
            _ => unreachable!(),
        }
        f.write(&db);
        let before = fs::read(f.database()).unwrap();
        assert_eq!(f.cli(&["policy", "enable"], None)["ok"], false, "{case}");
        assert_eq!(fs::read(f.database()).unwrap(), before);
    }
}
#[test]
fn unsafe_or_unknown_native_headers_cannot_create_or_clear_overrides() {
    for case in ["symlink", "writable", "unknown", "incomplete", "oversized"] {
        let f = Fixture::new();
        match case {
            "symlink" => {
                let original = f.root.path().join("original");
                fs::rename(&f.session, &original).unwrap();
                std::os::unix::fs::symlink(original, &f.session).unwrap();
            }
            "writable" => {
                fs::set_permissions(&f.session, fs::Permissions::from_mode(0o666)).unwrap()
            }
            "unknown" => fs::write(&f.session, "{\"type\":\"session\",\"version\":99}\n").unwrap(),
            "incomplete" => fs::write(&f.session, "{\"type\":\"session\"").unwrap(),
            "oversized" => fs::write(&f.session, "x".repeat(1024 * 1024 + 1)).unwrap(),
            _ => unreachable!(),
        }
        assert_eq!(f.cli(&["disable"], Some(&f.session))["ok"], false, "{case}");
        assert_eq!(
            f.cli(&["policy", "clear-disable"], Some(&f.session))["ok"],
            false,
            "{case}"
        );
        assert!(!f.database().exists());
    }
}
#[test]
fn concurrent_native_disables_cannot_lose_overrides() {
    let f = Fixture::new();
    let mut children = Vec::new();
    for index in 0..16 {
        let session = f.root.path().join(format!("session-{index}.jsonl"));
        fs::write(&session, format!("{}\n", json!({"type":"session","version":3,"id":format!("session-{index}"),"cwd":f.identity["cwd"]}))).unwrap();
        children.push(
            f.command()
                .args(["disable", "--session"])
                .arg(session)
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    for child in children {
        assert!(child.wait_with_output().unwrap().status.success());
    }
    let db = f.read();
    assert_eq!(
        db["policy"]["disabled_sessions"].as_object().unwrap().len(),
        16
    );
    assert!(db["records"].as_object().unwrap().is_empty());
}
#[test]
fn disable_does_not_discard_a_late_acceptance_receipt() {
    let f = Fixture::new();
    f.busy();
    let mut db = f.read();
    for record in db["records"].as_object_mut().unwrap().values_mut() {
        record["owner"]["boot"] = json!("00000000-0000-4000-8000-000000000001");
    }
    f.write(&db);
    let ticket = f.ok(&["recover"], Some(&f.session))["ticket"].clone();
    let mut open = f.owner_request("open");
    open["ticket"] = ticket.clone();
    assert_eq!(f.request(open)["ok"], true);
    f.ok(&["disable"], Some(&f.session));
    f.ok(&["policy", "disable"], None);
    let mut accepted = f.owner_request("accepted");
    accepted["attempt"] = ticket;
    assert_eq!(f.request(accepted)["ok"], true);
    let record = f.read()["records"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    assert_eq!(record["attempt"]["status"], "accepted");
    assert_eq!(record["enabled"], false);
    f.ok(&["policy", "enable"], None);
    f.ok(&["policy", "clear-disable"], Some(&f.session));
    assert_eq!(
        f.read()["records"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap(),
        &record
    );
}
#[test]
fn a_different_native_id_cannot_clear_an_existing_session_disable() {
    let f = Fixture::new();
    f.ok(&["disable"], Some(&f.session));
    let original = f.root.path().join("original-copy.jsonl");
    fs::copy(&f.session, &original).unwrap();
    fs::write(
        &f.session,
        format!(
            "{}\n",
            json!({"type":"session","version":3,"id":"new-session","cwd":f.identity["cwd"]})
        ),
    )
    .unwrap();
    assert_eq!(
        f.ok(&["policy", "clear-disable"], Some(&f.session))["cleared"],
        false
    );
    assert_eq!(
        f.ok(&["policy", "show"], Some(&original))["policy_enabled_for_session"],
        false
    );
}
#[test]
fn opting_out_does_not_need_to_scan_a_large_history() {
    let f = Fixture::new();
    let length = 128 * 1024 * 1024 + 1;
    fs::OpenOptions::new()
        .write(true)
        .open(&f.session)
        .unwrap()
        .set_len(length)
        .unwrap();
    f.ok(&["disable"], Some(&f.session));
    assert_eq!(
        f.ok(&["policy", "show"], Some(&f.session))["policy_enabled_for_session"],
        false
    );
    assert_eq!(fs::metadata(&f.session).unwrap().len(), length);
    assert!(f.read()["records"].as_object().unwrap().is_empty());
}
#[test]
fn duplicate_record_keys_cannot_silently_erase_an_uncertain_attempt() {
    let f = Fixture::new();
    let legacy = f.legacy(|record| {
        record["attempt"] = json!({"id":"00000000-0000-4000-8000-000000000002","boot":"00000000-0000-4000-8000-000000000003","expires":123,"status":"claimed"});
    });
    let (key, original) = legacy["records"]
        .as_object()
        .unwrap()
        .iter()
        .next()
        .unwrap();
    let mut replacement = original.clone();
    replacement["attempt"] = Value::Null;
    let bytes =
        format!("{{\"schema\":1,\"records\":{{\"{key}\":{original},\"{key}\":{replacement}}}}}");
    fs::write(f.database(), &bytes).unwrap();
    assert_eq!(f.cli(&["policy", "enable"], None)["ok"], false);
    assert_eq!(fs::read_to_string(f.database()).unwrap(), bytes);
}
#[test]
fn oversized_migration_is_blocked_without_poisoning_the_original() {
    let f = Fixture::new();
    let mut legacy = f.legacy(|record| {
        record["enabled"] = json!(false);
    });
    let mut record = legacy["records"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    // Valid old serialized shape, but duplicating its provenance exceeds the store cap.
    let huge_path = format!("/{}", "a".repeat(8 * 1024 * 1024));
    record["identity"]["session_file"] = json!(huge_path);
    legacy["records"] = json!({format!("{:x}",Sha256::digest(huge_path.as_bytes())):record});
    f.write(&legacy);
    let before = fs::read(f.database()).unwrap();
    assert!(before.len() < 16 * 1024 * 1024);
    let result = f.cli(&["policy", "disable"], None);
    assert_eq!(result["ok"], false);
    assert!(result["error"].as_str().unwrap().contains("byte limit"));
    assert_eq!(fs::read(f.database()).unwrap(), before);
}
