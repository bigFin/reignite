#![cfg(target_os = "linux")]
//! Protocol fixtures prove delivery mechanics, not native automatic eligibility.
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
    session: PathBuf,
    program: PathBuf,
    identity: Value,
    ticket: String,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("session with spaces.jsonl");
        let cwd = fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
        fs::write(&session, format!("{}\n{}\n", json!({"type":"session","version":3,"id":"native-session","cwd":cwd}),
            json!({"type":"message","id":"leaf1","parentId":null,"message":{"role":"user","content":"PRIVATE-PI-RPC-CANARY"}}))).unwrap();
        fs::set_permissions(&session, fs::Permissions::from_mode(0o600)).unwrap();
        let program = root.path().join("pi peer with spaces");
        // Nix build sandboxes have no /usr/bin/env. Use the pinned check input,
        // resolved before spawn, instead of relying on a host filesystem path.
        let python = std::env::split_paths(&std::env::var_os("PATH").expect("PATH"))
            .map(|directory| directory.join("python3"))
            .find(|path| path.is_file())
            .expect("Python test fixture requires python3")
            .canonicalize()
            .unwrap();
        fs::write(
            &program,
            format!(
                "#!{}\n{}",
                python.display(),
                include_str!("fixtures/pi-rpc-peer.py")
            ),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        let identity = json!({"harness":"pi","session_id":"native-session","session_file":session,"cwd":cwd,"leaf":"leaf1"});
        let mut f = Self {
            root,
            session,
            program,
            identity,
            ticket: String::new(),
        };
        let mut open = f.owner("open");
        open["ticket"] = Value::Null;
        f.request(open);
        f.request(f.owner("enable"));
        let mut busy = f.owner("observe");
        busy["activity"] = json!("busy");
        f.request(busy);
        f.edit(|db| {
            for record in db["records"].as_object_mut().unwrap().values_mut() {
                record["owner"]["boot"] = json!("00000000-0000-4000-8000-000000000001");
            }
        });
        f.ticket =
            f.request(json!({"op":"recover","session_file":f.session,"dry_run":false}))["ticket"]
                .as_str()
                .unwrap()
                .to_owned();
        f
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_reignite"));
        command
            .arg("--state-dir")
            .arg(self.root.path().join("state"));
        command
    }
    fn owner(&self, op: &str) -> Value {
        json!({"op":op,"identity":self.identity,"pid":std::process::id()})
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
    fn database(&self) -> Value {
        serde_json::from_slice(&fs::read(self.root.path().join("state/state.json")).unwrap())
            .unwrap()
    }
    fn edit(&self, edit: impl FnOnce(&mut Value)) {
        let mut db = self.database();
        edit(&mut db);
        fs::write(self.root.path().join("state/state.json"), db.to_string()).unwrap();
    }
    fn deliver(&self, case: &str, timeout: &str) -> (bool, Value) {
        let output = self
            .command()
            .args(["deliver-pi", "--session"])
            .arg(&self.session)
            .args(["--ticket", &self.ticket, "--pi-program"])
            .arg(&self.program)
            .args(["--timeout-ms", timeout, "--", "--model", case])
            .output()
            .unwrap();
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE-PI-RPC-CANARY"));
        (
            output.status.success(),
            serde_json::from_slice(&output.stdout).unwrap(),
        )
    }
    fn status(&self) -> String {
        self.database()["records"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()["attempt"]["status"]
            .as_str()
            .unwrap()
            .to_owned()
    }
    fn requests(&self) -> Vec<Value> {
        fs::read_to_string(self.root.path().join("peer-requests"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    fn assert_closed_and_no_answers(&self) {
        if let Ok(pid) = fs::read_to_string(self.root.path().join("peer-pid")) {
            assert!(
                !PathBuf::from(format!("/proc/{}", pid.trim())).exists(),
                "owned peer was not reaped"
            );
        }
        for request in self.requests() {
            assert!(
                matches!(
                    request["type"].as_str(),
                    Some("get_state" | "get_entries" | "get_messages" | "prompt")
                ),
                "{request}"
            );
        }
    }
}
#[test]
fn one_fixed_delivery_claims_before_write_and_never_repeats() {
    for case in ["normal", "fragmented"] {
        let f = Fixture::new();
        let (ok, value) = f.deliver(case, "2000");
        assert!(ok, "{value}");
        assert_eq!(value["result"]["outcome"], "settled");
        assert_eq!(value["result"]["api_acceptance_observed"], true);
        assert_eq!(value["result"]["native_eligibility_verified"], false);
        assert_eq!(value["result"]["work_complete_verified"], false);
        assert_eq!(f.status(), "accepted");
        let requests = f.requests();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["type"] == "prompt")
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["type"] == "get_state")
                .count(),
            2
        );
        let (ok, _) = f.deliver(case, "1000");
        assert!(!ok);
        assert_eq!(f.requests().len(), requests.len());
        f.assert_closed_and_no_answers();
    }
}
#[test]
fn questions_and_unknown_tool_outcomes_hold_before_claim() {
    for case in [
        "dialog",
        "history_question",
        "pending_tool",
        "tool_error",
        "startup_work",
        "changed_file",
    ] {
        let f = Fixture::new();
        let (ok, value) = f.deliver(case, "2000");
        assert!(ok, "{case}: {value}");
        assert_eq!(value["result"]["outcome"], "held");
        assert_eq!(f.status(), "authorized");
        assert!(
            f.requests()
                .iter()
                .all(|request| request["type"] != "prompt")
        );
        f.assert_closed_and_no_answers();
    }
}
#[test]
fn ended_assistant_turn_is_not_treated_as_permission_to_continue() {
    let f = Fixture::new();
    let cwd = f.identity["cwd"].clone();
    fs::write(&f.session, format!("{}\n{}\n", json!({"type":"session","version":3,"id":"native-session","cwd":cwd}),
        json!({"type":"message","id":"leaf1","parentId":null,"message":{"role":"assistant","stopReason":"stop","content":"Choose a branch before we proceed"}}))).unwrap();
    let (ok, value) = f.deliver("normal", "2000");
    assert!(ok, "{value}");
    assert_eq!(value["result"]["outcome"], "held");
    assert!(!f.root.path().join("peer-pid").exists());
    assert_eq!(f.status(), "authorized");
}
#[test]
fn protocol_identity_and_budget_failures_never_claim_or_disclose() {
    for case in [
        "wrong_session",
        "wrong_file",
        "wrong_branch",
        "wrong_correlation",
        "busy",
        "corrupt",
        "partial",
        "oversized",
        "stderr_flood",
        "unknown_ui",
        "rejected",
        "timeout",
        "trickle",
        "disable_after_load",
        "expire_after_load",
    ] {
        let f = Fixture::new();
        let started = Instant::now();
        let (ok, value) = f.deliver(
            case,
            if matches!(case, "timeout" | "trickle") {
                "100"
            } else {
                "2000"
            },
        );
        assert!(!ok, "{case}: {value}");
        assert!(started.elapsed() < Duration::from_secs(4));
        assert_eq!(f.status(), "authorized", "{case}: {value}");
        assert!(
            f.requests()
                .iter()
                .all(|request| request["type"] != "prompt")
        );
        f.assert_closed_and_no_answers();
    }
}
#[test]
fn lost_ack_and_postsubmission_holds_keep_consumed_delivery() {
    for case in [
        "lost_ack",
        "prompt_dialog",
        "run_dialog",
        "run_error",
        "queued",
        "handled",
        "end_only",
    ] {
        let f = Fixture::new();
        let (_, value) = f.deliver(case, "500");
        assert!(
            matches!(f.status().as_str(), "claimed" | "accepted"),
            "{case}: {value}"
        );
        if case == "lost_ack" || case == "prompt_dialog" {
            assert_eq!(f.status(), "claimed");
        } else {
            assert_eq!(f.status(), "accepted");
        }
        let requests = f.requests();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["type"] == "prompt")
                .count(),
            1
        );
        let (ok, _) = f.deliver(case, "500");
        assert!(!ok);
        assert_eq!(requests.len(), f.requests().len());
        f.assert_closed_and_no_answers();
    }
}
#[test]
fn disable_and_unreviewed_profile_arguments_prevent_launch() {
    let f = Fixture::new();
    for tail in [
        vec!["saved task"],
        vec!["@file"],
        vec!["--session", "other"],
        vec!["--mode", "text"],
        vec!["--recovery-ticket", "other"],
        vec!["--model"],
    ] {
        let output = f
            .command()
            .args(["deliver-pi", "--session"])
            .arg(&f.session)
            .args(["--ticket", &f.ticket, "--pi-program"])
            .arg(&f.program)
            .arg("--")
            .args(tail)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!f.root.path().join("peer-pid").exists());
        assert_eq!(f.status(), "authorized");
    }
    f.request(json!({"op":"set_host_policy","enabled":false}));
    let (ok, _) = f.deliver("normal", "1000");
    assert!(!ok);
    assert!(!f.root.path().join("peer-pid").exists());
    f.request(json!({"op":"set_host_policy","enabled":true}));
    f.request(json!({"op":"disable","session_file":f.session}));
    let (ok, _) = f.deliver("normal", "1000");
    assert!(!ok);
    assert!(!f.root.path().join("peer-pid").exists());
}

#[test]
fn host_default_does_not_authorize_unregistered_native_delivery() {
    let f = Fixture::new();
    f.edit(|database| database["records"] = json!({}));
    let (ok, value) = f.deliver("normal", "1000");
    assert!(!ok, "{value}");
    assert!(!f.root.path().join("peer-pid").exists());
}
