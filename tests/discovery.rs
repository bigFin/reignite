use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

struct Fixture {
    temp: TempDir,
    home: PathBuf,
    root: PathBuf,
    cwd: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home with spaces");
        let root = home.join(".pi/agent/sessions");
        let cwd = temp.path().join("project");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir(&cwd).unwrap();
        Self {
            temp,
            home,
            root,
            cwd,
        }
    }
    fn state(&self) -> PathBuf {
        self.temp.path().join("state/state.json")
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_reignite"));
        c.env("HOME", &self.home)
            .env_remove("PI_CODING_AGENT_DIR")
            .env_remove("PI_CODING_AGENT_SESSION_DIR")
            .env_remove("PI_SUBAGENTS_TEMP_ROOT")
            .arg("--state-dir")
            .arg(self.temp.path().join("state"));
        c
    }
    fn cli(&self, args: &[&str]) -> Output {
        let before = fs::read(self.state()).ok();
        let output = self.command().args(args).output().unwrap();
        assert_eq!(fs::read(self.state()).ok(), before);
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE-CANARY"));
        output
    }
    fn report(&self) -> Value {
        let output = self.cli(&["discover", "--json"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        let report = value["result"].clone();
        assert_eq!(report["automatic_recovery_implemented"], false);
        assert_eq!(report["boot_change_verified"], false);
        assert!(
            report["findings"]
                .as_array()
                .unwrap()
                .iter()
                .all(|finding| matches!(
                    finding["gate_decision"].as_str(),
                    Some("blocked" | "hold")
                ))
        );
        report
    }
    fn history(&self, root: &Path, name: &str, native_id: &str, stop: Option<&str>) -> PathBuf {
        fs::create_dir_all(root).unwrap();
        let file = root.join(name);
        let mut text = format!(
            "{}\n{}\n",
            json!({"type":"session","version":3,"id":native_id,"cwd":self.cwd}),
            json!({"type":"message","id":"user1","parentId":null,"message":{"role":"user","content":"PRIVATE-CANARY"}})
        );
        if let Some(stop) = stop {
            text.push_str(&format!("{}\n", json!({"type":"message","id":"assistant1","parentId":"user1","message":{"role":"assistant","stopReason":stop,"content":[{"type":"text","text":"PRIVATE-CANARY"}]}})));
        }
        fs::write(&file, text).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        file
    }
}

#[test]
fn discovery_needs_no_enrollment_and_defaults_to_plain_english() {
    let f = Fixture::new();
    let folder = f.root.join("--workspace--");
    let cancelled = f.history(&folder, "cancelled.jsonl", "cancelled-id", Some("aborted"));
    let uncertain = f.history(&folder, "uncertain.jsonl", "uncertain-id", None);
    f.history(&folder, "ended.jsonl", "ended-id", Some("stop"));
    let before = fs::read(&cancelled).unwrap();
    let uncertain_before = fs::read(&uncertain).unwrap();
    let output = f.cli(&["discover"]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Nothing was restarted"));
    assert!(text.contains("Saved history reports cancellation"));
    assert!(text.contains("Your decision is needed"));
    assert!(!text.starts_with('{'));
    let report = f.report();
    assert_eq!(report["findings"].as_array().unwrap().len(), 3);
    assert_eq!(report["distinct_native_ids"], 3);
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["category"] == "recorded_cancellation")
    );
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["category"] == "needs_review")
    );
    assert_eq!(fs::read(cancelled).unwrap(), before);
    assert_eq!(fs::read(uncertain).unwrap(), uncertain_before);
    assert!(!f.state().exists());
}

#[test]
fn aliases_and_nested_native_files_are_visible_without_double_authorization() {
    let f = Fixture::new();
    let original = f.history(
        &f.root.join("--workspace--"),
        "parent.jsonl",
        "shared-id",
        None,
    );
    let nested = f.root.join("--workspace--/parent/run-0");
    let alias = f.history(&nested, "session.jsonl", "shared-id", None);
    let report = f.report();
    assert_eq!(report["findings"].as_array().unwrap().len(), 2);
    assert_eq!(report["distinct_native_ids"], 1);
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["shared_native_identity"] == true)
    );
    let output = f
        .command()
        .args(["disable", "--session"])
        .arg(&original)
        .output()
        .unwrap();
    assert!(output.status.success());
    let state = fs::read(f.state()).unwrap();
    let report = f.report();
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["gate_decision"] == "blocked")
    );
    assert_eq!(fs::read(f.state()).unwrap(), state);
    assert_eq!(fs::read(original).unwrap(), fs::read(alias).unwrap());
}

#[test]
fn native_environment_locations_and_explicit_roots_do_not_change_configuration() {
    let f = Fixture::new();
    let agent = f.temp.path().join("alternative-agent");
    let custom = f.temp.path().join("custom-sessions");
    f.history(
        &agent.join("sessions/--workspace--"),
        "a.jsonl",
        "agent-id",
        None,
    );
    f.history(&custom, "b.jsonl", "custom-id", None);
    let output = f
        .command()
        .env("PI_CODING_AGENT_DIR", &agent)
        .env("PI_CODING_AGENT_SESSION_DIR", &custom)
        .args(["discover", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["result"]["findings"].as_array().unwrap().len(), 2);
    let output = f
        .command()
        .args(["discover", "--json", "--sessions-root"])
        .arg(&custom)
        .arg("--sessions-root")
        .arg(&custom)
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["result"]["findings"].as_array().unwrap().len(), 1);
    assert!(!agent.join("settings.json").exists());
    assert!(!f.state().exists());
}

#[test]
fn unsafe_links_and_damaged_histories_stay_visible_without_repair() {
    let f = Fixture::new();
    let good = f.history(&f.root, "good.jsonl", "good-id", None);
    symlink(&good, f.root.join("linked.jsonl")).unwrap();
    symlink(&f.cwd, f.root.join("linked-dir")).unwrap();
    let damaged = f.root.join("damaged.jsonl");
    fs::write(&damaged, "{PRIVATE-CANARY}").unwrap();
    let unsafe_dir = f.root.join("unsafe-directory");
    f.history(&unsafe_dir, "unsafe.jsonl", "unsafe-id", None);
    fs::set_permissions(&unsafe_dir, fs::Permissions::from_mode(0o777)).unwrap();
    let report = f.report();
    assert_eq!(report["findings"].as_array().unwrap().len(), 2);
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["category"] == "unreadable")
    );
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["code"] == "symlink_not_followed")
    );
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["code"] == "unsafe_or_noncanonical_directory")
    );
    assert_eq!(fs::read_to_string(damaged).unwrap(), "{PRIVATE-CANARY}");
    assert!(!f.state().exists());
}

#[test]
fn missing_roots_and_missing_workspaces_are_not_proof_of_no_interrupted_work() {
    let f = Fixture::new();
    f.history(&f.root, "gone-workspace.jsonl", "gone-id", None);
    fs::remove_dir(&f.cwd).unwrap();
    let report = f.report();
    assert_eq!(report["findings"][0]["category"], "needs_review");
    fs::remove_dir_all(&f.root).unwrap();
    let report = f.report();
    assert!(report["findings"].as_array().unwrap().is_empty());
    assert!(!report["warnings"].as_array().unwrap().is_empty());
    let output = f.cli(&["discover"]);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("does not prove there was no interrupted work"));
    assert!(!f.root.exists());
}

#[test]
fn bounds_on_file_count_depth_entries_and_bytes_report_partial_coverage() {
    let f = Fixture::new();
    for i in 0..257 {
        f.history(&f.root, &format!("s{i:03}.jsonl"), &format!("id{i}"), None);
    }
    let report = f.report();
    assert_eq!(report["findings"].as_array().unwrap().len(), 256);
    assert_eq!(report["truncated"], true);
    fs::remove_dir_all(&f.root).unwrap();
    fs::create_dir_all(&f.root).unwrap();
    let deep = f.root.join("a/b/c/d/e/f/g/h/i");
    f.history(&deep, "deep.jsonl", "deep-id", None);
    let report = f.report();
    assert_eq!(report["truncated"], true);
    assert!(report["findings"].as_array().unwrap().is_empty());
    fs::remove_dir_all(&f.root).unwrap();
    fs::create_dir_all(&f.root).unwrap();
    for i in 0..4097 {
        fs::write(f.root.join(format!("ignored{i}")), "").unwrap();
    }
    let report = f.report();
    assert_eq!(report["directory_entries_seen"], 4096);
    assert_eq!(report["truncated"], true);
    fs::remove_dir_all(&f.root).unwrap();
    fs::create_dir_all(&f.root).unwrap();
    let huge = f.root.join("huge.jsonl");
    let file = fs::File::create(&huge).unwrap();
    file.set_len(128 * 1024 * 1024 + 1).unwrap();
    let report = f.report();
    assert_eq!(report["truncated"], true);
    assert_eq!(report["session_bytes_reserved"], 0);
    assert_eq!(report["findings"][0]["category"], "unreadable");
    fs::remove_file(huge).unwrap();
    // Failed reads also consume reservations, so a corrupt file cannot bypass
    // the aggregate bound and trigger repeated large inspections.
    for name in ["a-corrupt.jsonl", "b-corrupt.jsonl"] {
        use std::io::Write;
        let mut file = fs::File::create(f.root.join(name)).unwrap();
        file.write_all(b"{}\n").unwrap();
        file.set_len(64 * 1024 * 1024).unwrap();
    }
    f.history(&f.root, "c-valid.jsonl", "valid-id", None);
    let report = f.report();
    assert_eq!(report["session_bytes_reserved"], 128 * 1024 * 1024);
    assert_eq!(report["truncated"], true);
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["category"] == "unreadable")
    );
    fs::remove_dir_all(&f.root).unwrap();
    fs::create_dir_all(&f.root).unwrap();
    for i in 0..513 {
        fs::create_dir(f.root.join(format!("dir{i:03}"))).unwrap();
    }
    let report = f.report();
    assert_eq!(report["directories_read"], 512);
    assert_eq!(report["truncated"], true);
    assert!(!f.state().exists());
}

#[test]
fn plain_output_escapes_terminal_controls_in_paths() {
    let f = Fixture::new();
    f.history(&f.root, "line\n\u{1b}[31m.jsonl", "escape-id", None);
    let output = f.cli(&["discover"]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains('\u{1b}'));
    assert!(text.contains("\\n\\u{1b}[31m.jsonl"));
}

#[test]
fn corrupt_policy_is_fatal_in_plain_and_json_modes_and_original_is_retained() {
    let f = Fixture::new();
    let session = f.history(&f.root, "native.jsonl", "native-id", None);
    assert!(
        f.command()
            .args(["disable", "--session"])
            .arg(&session)
            .output()
            .unwrap()
            .status
            .success()
    );
    fs::write(f.state(), "{corrupt").unwrap();
    let output = f.cli(&["discover"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("Could not check saved sessions")
    );
    let output = f.cli(&["discover", "--json"]);
    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(fs::read_to_string(f.state()).unwrap(), "{corrupt");
}

#[test]
fn json_request_and_schema_one_attempts_are_read_without_migration_or_reset() {
    use std::{io::Write, process::Stdio};
    let mut f = Fixture::new();
    // Legacy owner validation checks the real test process's working directory.
    f.cwd = fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
    let session = f.history(&f.root, "native.jsonl", "native-id", None);
    let request = |value: Value| {
        let mut child = f
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
            .write_all(value.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["result"].clone()
    };
    let identity = json!({"harness":"pi","session_id":"native-id","session_file":session,"cwd":f.cwd,"leaf":"user1"});
    request(json!({"op":"open","identity":identity,"pid":std::process::id(),"ticket":null}));
    request(json!({"op":"enable","identity":identity,"pid":std::process::id()}));
    request(json!({"op":"observe","identity":identity,"pid":std::process::id(),"activity":"busy"}));
    let mut db: Value = serde_json::from_slice(&fs::read(f.state()).unwrap()).unwrap();
    db["schema"] = json!(1);
    db.as_object_mut().unwrap().remove("policy");
    for status in ["authorized", "claimed", "accepted"] {
        let row = db["records"]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .next()
            .unwrap();
        row["owner"]["boot"] = json!("00000000-0000-4000-8000-000000000001");
        row["attempt"] = json!({"id":"00000000-0000-4000-8000-000000000002","boot":"00000000-0000-4000-8000-000000000001","expires":0,"status":status});
        fs::write(f.state(), db.to_string()).unwrap();
        let before = fs::read(f.state()).unwrap();
        let report = f.report();
        assert_eq!(report["migration_pending"], true);
        assert_eq!(report["findings"][0]["gate_decision"], "blocked");
        assert_eq!(request(json!({"op":"discover_pi"}))["state_schema"], 1);
        assert_eq!(fs::read(f.state()).unwrap(), before);
    }
}
