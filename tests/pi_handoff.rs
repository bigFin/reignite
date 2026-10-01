#![cfg(target_os = "linux")]
//! Real CLI/PTY handoff boundary with a disposable native-client stand-in.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Write,
    os::{fd::FromRawFd, unix::fs::PermissionsExt},
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
    session: PathBuf,
    program: PathBuf,
}
struct Owned {
    child: Child,
    terminal: File,
}
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("session with spaces.jsonl");
        fs::write(&session, format!("{}\n{}\n", json!({"type":"session","version":3,"id":"native-id","cwd":root.path()}),
            json!({"type":"message","id":"user1","parentId":null,"message":{"role":"user","content":"PRIVATE-CANARY"}}))).unwrap();
        fs::set_permissions(&session, fs::Permissions::from_mode(0o600)).unwrap();
        let python = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|path| path.join("python3"))
            .find(|path| path.is_file())
            .unwrap()
            .canonicalize()
            .unwrap();
        let program = root.path().join("native client");
        fs::write(&program, format!("#!{}\nimport json,os,pathlib,sys\nassert all(os.isatty(fd) for fd in (0,1,2))\npathlib.Path('launched').write_text(json.dumps({{'pid':os.getpid(),'args':sys.argv[1:]}}))\nfor line in sys.stdin:\n if line.strip() == 'quit': break\n", python.display())).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            root,
            session,
            program,
        }
    }
    fn state(&self) -> PathBuf {
        self.root.path().join("state/state.json")
    }
    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_reignite"));
        cmd.arg("--state-dir").arg(self.root.path().join("state"));
        cmd
    }
    fn handoff(&self) -> Command {
        let mut cmd = self.command();
        cmd.args(["handoff-pi", "--session"])
            .arg(&self.session)
            .arg("--pi-program")
            .arg(&self.program);
        cmd
    }
    fn seed(&self, status: &str, live: bool) {
        let output = self
            .command()
            .args(["disable", "--session"])
            .arg(&self.session)
            .output()
            .unwrap();
        assert!(output.status.success());
        let mut db: Value = serde_json::from_slice(&fs::read(self.state()).unwrap()).unwrap();
        let meta = fs::metadata(&self.session).unwrap();
        use std::os::unix::fs::MetadataExt;
        let boot = if live {
            fs::read_to_string("/proc/sys/kernel/random/boot_id")
                .unwrap()
                .trim()
                .to_owned()
        } else {
            "00000000-0000-4000-8000-000000000001".into()
        };
        let stat = fs::read_to_string(format!("/proc/{}/stat", std::process::id())).unwrap();
        let start = stat
            .rsplit_once(')')
            .unwrap()
            .1
            .split_whitespace()
            .nth(19)
            .unwrap();
        db["records"][format!(
            "{:x}",
            Sha256::digest(self.session.as_os_str().as_encoded_bytes())
        )] = json!({
            "identity":{"harness":"pi","session_id":"native-id","session_file":self.session,"cwd":self.root.path(),"leaf":"user1"},
            "device":meta.dev(),"inode":meta.ino(),"owner":{"boot":boot,"pid":std::process::id(),"start":start},
            "enabled":false,"activity":"stopped","shutdown_ambiguous":true,
            "attempt":{"id":"00000000-0000-4000-8000-000000000002","boot":boot,"expires":0,"status":status}});
        fs::write(self.state(), db.to_string()).unwrap();
    }
    fn start(&self, args: &[&str]) -> Owned {
        let (mut master, mut slave) = (0, 0);
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                )
            },
            0
        );
        let terminal = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        let child = self
            .handoff()
            .args(args)
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave))
            .spawn()
            .unwrap();
        Owned { child, terminal }
    }
    fn launched(&self, owned: &mut Owned) -> Value {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(bytes) = fs::read(self.root.path().join("launched"))
                && let Ok(value) = serde_json::from_slice(&bytes)
            {
                return value;
            }
            assert!(
                owned.child.try_wait().unwrap().is_none(),
                "handoff exited before native client"
            );
            thread::sleep(Duration::from_millis(10));
        }
        panic!("native client not reached");
    }
}
#[test]
fn handoff_is_terminal_only_and_cannot_become_a_headless_delivery_route() {
    let f = Fixture::new();
    let output = f.handoff().output().unwrap();
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE-CANARY"));
    assert!(!f.root.path().join("launched").exists());
    assert!(!f.state().exists());
}
#[test]
fn manual_exec_preserves_pid_profile_disable_and_all_attempt_states() {
    for status in ["authorized", "claimed", "accepted"] {
        let f = Fixture::new();
        f.seed(status, false);
        let before = fs::read(f.state()).unwrap();
        let history = fs::read(&f.session).unwrap();
        let mut owned = f.start(&["--", "--model", "fixture", "--no-tools"]);
        let value = f.launched(&mut owned);
        assert_eq!(value["pid"], owned.child.id());
        assert_eq!(
            value["args"],
            json!(["--model", "fixture", "--no-tools", "--session", f.session])
        );
        assert_eq!(fs::read(f.state()).unwrap(), before);
        assert_eq!(fs::read(&f.session).unwrap(), history);
        owned.terminal.write_all(b"quit\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while owned.child.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(fs::read(f.state()).unwrap(), before);
    }
}
#[test]
fn live_retained_owner_and_unreviewed_inputs_prevent_interactive_launch() {
    let f = Fixture::new();
    f.seed("claimed", true);
    let before = fs::read(f.state()).unwrap();
    let mut owned = f.start(&[]);
    assert!(!owned.child.wait().unwrap().success());
    assert!(!f.root.path().join("launched").exists());
    assert_eq!(fs::read(f.state()).unwrap(), before);
    for args in [
        vec!["--", "replayed task"],
        vec!["--", "@file"],
        vec!["--", "--mode", "rpc"],
        vec!["--", "--session", "other"],
    ] {
        let f = Fixture::new();
        let mut owned = f.start(&args);
        assert!(!owned.child.wait().unwrap().success());
        assert!(!f.root.path().join("launched").exists());
    }
}
