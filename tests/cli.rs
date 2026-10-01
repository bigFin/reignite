//! CLI boundary tests. Previous boots are simulated by editing only disposable records.
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
use tempfile::TempDir;

struct Fixture {
    temp: TempDir,
    identity: Value,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let session = temp.path().join("session with spaces.jsonl");
        let cwd = fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
        fs::write(&session, format!("{}\n{}\n", json!({"type":"session","version":3,"id":"test-session","cwd":cwd}), json!({"type":"message","id":"leaf1","parentId":null,"message":{"role":"user","content":"disposable"}}))).unwrap();
        Self {
            temp,
            identity: json!({"harness":"pi","session_id":"test-session","session_file":session,"cwd":cwd,"leaf":"leaf1"}),
        }
    }
    fn state(&self) -> PathBuf {
        self.temp.path().join("state")
    }
    fn cmd(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_reignite"));
        c.arg("--state-dir").arg(self.state());
        c
    }
    fn run(&self, request: Value) -> Value {
        let mut child = self
            .cmd()
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
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output.status.success(), response["ok"] == true);
        response
    }
    fn ok(&self, request: Value) -> Value {
        let r = self.run(request);
        assert_eq!(r["ok"], true, "{r}");
        r["result"].clone()
    }
    fn owner(&self, op: &str) -> Value {
        json!({"op":op,"identity":self.identity,"pid":std::process::id()})
    }
    fn open(&self, ticket: Value) -> Value {
        let mut r = self.owner("open");
        r["ticket"] = ticket;
        r
    }
    fn observe(&self, activity: &str) {
        let mut r = self.owner("observe");
        r["activity"] = json!(activity);
        self.ok(r);
    }
    fn busy(&self) {
        self.ok(self.open(Value::Null));
        self.ok(self.owner("enable"));
        self.observe("busy");
    }
    fn edit(&self, f: impl FnOnce(&mut Value)) {
        let p = self.state().join("state.json");
        let mut db: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        f(&mut db);
        fs::write(p, db.to_string()).unwrap();
    }
    fn old_boot(&self) {
        self.edit(|db| {
            for r in db["records"].as_object_mut().unwrap().values_mut() {
                r["owner"]["boot"] = json!("00000000-0000-4000-8000-000000000001");
            }
        });
    }
    fn recover(&self, dry_run: bool) -> Value {
        json!({"op":"recover","session_file":self.identity["session_file"],"dry_run":dry_run})
    }
    fn record(&self) -> Value {
        self.ok(json!({"op":"status"}))["records"][0].clone()
    }
}

#[test]
fn explicit_restore_claim_is_single_use_and_uncertainty_survives_another_boot() {
    let f = Fixture::new();
    f.busy();
    assert_eq!(f.ok(f.recover(true))["eligible"], false);
    f.old_boot();
    assert_eq!(f.ok(f.recover(true))["eligible"], true);
    let ticket = f.ok(f.recover(false))["ticket"].clone();
    let claimed = f.ok(f.open(ticket.clone()));
    assert!(
        claimed["continuation"]
            .as_str()
            .unwrap()
            .contains("Unknown tool delivery")
    );
    assert_eq!(f.record()["attempt"]["status"], "claimed");
    assert_eq!(f.run(f.open(ticket))["ok"], false);
    // No acknowledgement occurred: delivery may have happened, and is never retried.
    f.old_boot();
    assert_eq!(f.ok(f.recover(true))["eligible"], false);
    assert_eq!(f.run(f.recover(false))["ok"], false);
}

#[test]
fn idle_waiting_stopped_disabled_and_ambiguous_shutdown_block_recovery() {
    for activity in ["idle", "waiting", "stopped", "disabled", "shutdown"] {
        let f = Fixture::new();
        f.busy();
        match activity {
            "disabled" => {
                f.ok(json!({"op":"disable","session_file":f.identity["session_file"]}));
                f.observe("busy");
            }
            "shutdown" => {
                f.ok(f.owner("shutdown"));
            }
            other => f.observe(other),
        }
        f.old_boot();
        let result = f.ok(f.recover(true));
        assert_eq!(result["eligible"], false, "{activity}: {result}");
        if activity == "shutdown" {
            assert!(
                result["blocked_reason"]
                    .as_str()
                    .unwrap()
                    .contains("ambiguous")
            );
            assert_eq!(f.record()["activity"], "busy");
        }
    }
}

#[test]
fn manual_reopen_disarms_and_tickets_expire_or_mismatch_without_delivery() {
    for case in ["manual", "expired", "wrong", "branch", "header", "inode"] {
        let f = Fixture::new();
        f.busy();
        f.old_boot();
        let mut ticket = f.ok(f.recover(false))["ticket"].clone();
        let mut open = f.open(ticket.clone());
        match case {
            "manual" => {
                let r = f.ok(f.open(Value::Null));
                assert!(r["continuation"].is_null());
                assert_eq!(r["record"]["enabled"], false);
            }
            "expired" => f.edit(|db| {
                for r in db["records"].as_object_mut().unwrap().values_mut() {
                    r["attempt"]["expires"] = json!(0);
                }
            }),
            "wrong" => {
                ticket = json!("00000000-0000-4000-8000-000000000002");
                open["ticket"] = ticket;
            }
            "branch" => {
                open["identity"]["leaf"] = Value::Null;
            }
            "header" => {
                open["identity"]["session_id"] = json!("different");
            }
            "inode" => {
                let path = f.identity["session_file"].as_str().unwrap();
                let content = fs::read(path).unwrap();
                fs::rename(path, format!("{path}.old")).unwrap();
                fs::write(path, content).unwrap();
            }
            _ => unreachable!(),
        }
        assert_eq!(f.run(open)["ok"], false, "{case}");
    }
}

#[test]
fn concurrent_subprocesses_cannot_duplicate_claim_or_steal_live_session() {
    let f = Fixture::new();
    f.busy();
    let mut other = Command::new("sleep").arg("30").spawn().unwrap();
    let mut steal = f.open(Value::Null);
    steal["pid"] = json!(other.id());
    assert_eq!(f.run(steal)["ok"], false);
    other.kill().unwrap();
    other.wait().unwrap();
    f.old_boot();
    let ticket = f.ok(f.recover(false))["ticket"].clone();
    let mut children = Vec::new();
    for _ in 0..12 {
        let mut child = f
            .cmd()
            .arg("request")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(f.open(ticket.clone()).to_string().as_bytes())
            .unwrap();
        children.push(child);
    }
    let successful = children
        .into_iter()
        .filter_map(|c| {
            let out = c.wait_with_output().unwrap();
            out.status.success().then_some(out)
        })
        .count();
    assert_eq!(successful, 1);
    assert_eq!(f.record()["attempt"]["status"], "claimed");
}

#[test]
fn corrupt_unknown_or_nonprivate_state_fails_closed_without_replacing_it() {
    for case in [
        "json",
        "schema",
        "field",
        "permissions",
        "symlink",
        "dangling_symlink",
    ] {
        let f = Fixture::new();
        f.busy();
        let p = f.state().join("state.json");
        match case {
            "json" => fs::write(&p, "{broken").unwrap(),
            "schema" => f.edit(|db| db["schema"] = json!(3)),
            "field" => f.edit(|db| db["unknown"] = json!(true)),
            "permissions" => {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
            }
            "symlink" | "dangling_symlink" => {
                let moved = f.state().join("original");
                fs::rename(&p, &moved).unwrap();
                let target = if case == "symlink" {
                    moved
                } else {
                    f.state().join("missing")
                };
                std::os::unix::fs::symlink(target, &p).unwrap();
            }
            _ => unreachable!(),
        }
        let bytes = fs::read(&p).ok();
        let link = fs::read_link(&p).ok();
        assert_eq!(f.run(json!({"op":"status"}))["ok"], false, "{case}");
        assert_eq!(f.run(f.open(Value::Null))["ok"], false, "{case}");
        assert_eq!(fs::read(&p).ok(), bytes);
        assert_eq!(fs::read_link(&p).ok(), link);
    }
}

#[test]
fn practical_commands_and_api_acceptance_keep_attempt_visible() {
    let f = Fixture::new();
    f.busy();
    f.old_boot();
    let out = f
        .cmd()
        .arg("recover")
        .arg("--session")
        .arg(f.identity["session_file"].as_str().unwrap())
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&out.stdout).unwrap()["result"]["eligible"],
        true
    );
    let ticket = f.ok(f.recover(false))["ticket"].clone();
    f.ok(f.open(ticket.clone()));
    let mut accepted = f.owner("accepted");
    accepted["attempt"] = ticket;
    f.ok(accepted);
    f.observe("idle");
    assert_eq!(f.record()["attempt"]["status"], "accepted");
    let out = f
        .cmd()
        .arg("disable")
        .arg("--session")
        .arg(f.identity["session_file"].as_str().unwrap())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(f.record()["enabled"], false);
}

#[test]
fn ticket_expiring_while_waiting_for_the_store_lock_is_not_claimed() {
    use fs2::FileExt;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
    let f = Fixture::new();
    f.busy();
    f.old_boot();
    let ticket = f.ok(f.recover(false))["ticket"].clone();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(f.state().join("lock"))
        .unwrap();
    lock.lock_exclusive().unwrap();
    let mut child = f
        .cmd()
        .arg("request")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(f.open(ticket).to_string().as_bytes())
        .unwrap();
    let started = Instant::now();
    while !fs::read_to_string(format!("/proc/{}/wchan", child.id()))
        .is_ok_and(|channel| channel.contains("locks_lock"))
    {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "claim did not reach the held lock"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let expires = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 1;
    f.edit(|db| {
        for record in db["records"].as_object_mut().unwrap().values_mut() {
            record["attempt"]["expires"] = json!(expires);
        }
    });
    std::thread::sleep(Duration::from_secs(2));
    FileExt::unlock(&lock).unwrap();
    let output = child.wait_with_output().unwrap();
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        !output.status.success(),
        "expired claim succeeded: {response}"
    );
    assert!(
        response["error"]
            .as_str()
            .unwrap()
            .contains("stale/consumed/mismatched")
    );
    assert_eq!(f.record()["attempt"]["status"], "authorized");
}

#[test]
fn legacy_validation_uses_bounded_complete_private_native_sources() {
    use std::os::unix::fs::PermissionsExt;
    for case in [
        "large_file",
        "large_entry",
        "writable",
        "incomplete_null_leaf",
    ] {
        let f = Fixture::new();
        f.busy();
        let before = fs::read(f.state().join("state.json")).unwrap();
        let session = f.identity["session_file"].as_str().unwrap();
        let mut request = f.open(Value::Null);
        match case {
            "large_file" => fs::OpenOptions::new()
                .write(true)
                .open(session)
                .unwrap()
                .set_len(128 * 1024 * 1024 + 1)
                .unwrap(),
            "large_entry" => {
                let mut file = fs::OpenOptions::new().append(true).open(session).unwrap();
                writeln!(
                    file,
                    "{}",
                    json!({"type":"custom", "id":"large", "data":"x".repeat(1024 * 1024)})
                )
                .unwrap();
            }
            "writable" => fs::set_permissions(session, fs::Permissions::from_mode(0o666)).unwrap(),
            "incomplete_null_leaf" => {
                request["identity"]["leaf"] = Value::Null;
                fs::OpenOptions::new()
                    .append(true)
                    .open(session)
                    .unwrap()
                    .write_all(b"{\"type\":")
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assert_eq!(f.run(request)["ok"], false, "{case}");
        assert_eq!(
            fs::read(f.state().join("state.json")).unwrap(),
            before,
            "{case}"
        );
    }
}
