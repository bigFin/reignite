#![cfg(target_os = "linux")]
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::Path,
    process::{Command, Output},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;
use tungstenite::{Message, WebSocket};

const PRIVATE: &str = "PRIVATE-CONVERSATION-CANARY";
#[derive(Clone, Copy, Debug)]
enum Case {
    Good,
    ActiveApproval,
    UnknownState,
    UnknownFlag,
    WrongThread,
    WrongCorrelation,
    Corrupt,
    Oversized,
    InitRejected,
    ReadRejected,
    BadInit,
    ApprovalRequest,
    Disconnect,
    Timeout,
    Trickle,
    Flood,
    WireLimit,
}
fn record(socket: &mut WebSocket<UnixStream>, log: &mut Vec<Value>) -> Value {
    let value: Value = serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
    log.push(value.clone());
    value
}
fn send(socket: &mut WebSocket<UnixStream>, value: Value) {
    let _ = socket.send(Message::text(value.to_string()));
}
fn drain(socket: &mut WebSocket<UnixStream>, log: &mut Vec<Value>) {
    while let Ok(message) = socket.read() {
        if let Message::Text(text) = message {
            log.push(serde_json::from_str(&text).unwrap());
        }
    }
}
fn serve(stream: UnixStream, case: Case) -> Vec<Value> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut socket = tungstenite::accept(stream).unwrap();
    let mut log = Vec::new();
    let initialize = record(&mut socket, &mut log);
    assert_eq!(initialize["method"], "initialize");
    assert_eq!(
        initialize["params"]["capabilities"]["experimentalApi"],
        false
    );
    match case {
        Case::InitRejected => {
            send(
                &mut socket,
                json!({"id":initialize["id"],"error":{"code":-32601,"message":PRIVATE}}),
            );
            drain(&mut socket, &mut log);
            return log;
        }
        Case::BadInit => {
            send(
                &mut socket,
                json!({"id":initialize["id"],"result":{"other":PRIVATE}}),
            );
            drain(&mut socket, &mut log);
            return log;
        }
        Case::Disconnect => {
            let _ = socket.close(None);
            return log;
        }
        Case::Timeout | Case::Trickle => {
            if matches!(case, Case::Timeout) {
                thread::sleep(Duration::from_millis(250));
            } else {
                for _ in 0..20 {
                    if socket
                        .send(Message::text(
                            json!({"method":"notice","params":PRIVATE}).to_string(),
                        ))
                        .is_err()
                    {
                        break;
                    }
                    thread::sleep(Duration::from_millis(15));
                }
            }
            drain(&mut socket, &mut log);
            return log;
        }
        _ => {}
    }
    send(
        &mut socket,
        json!({"id":initialize["id"],"result":{"userAgent":PRIVATE}}),
    );
    let initialized = record(&mut socket, &mut log);
    assert_eq!(initialized, json!({"method":"initialized"}));
    let read = record(&mut socket, &mut log);
    assert_eq!(read["method"], "thread/read");
    assert_eq!(
        read["params"],
        json!({"threadId":"thread-exact","includeTurns":false})
    );
    match case {
        Case::Corrupt => {
            let _ = socket.send(Message::text("not JSON"));
        }
        Case::Oversized => {
            let _ = socket.send(Message::text("x".repeat(128 * 1024 + 1)));
        }
        Case::ApprovalRequest => send(
            &mut socket,
            json!({"id":"approval-1","method":"item/commandExecution/requestApproval","params":{"command":PRIVATE}}),
        ),
        Case::ReadRejected => send(
            &mut socket,
            json!({"id":read["id"],"error":{"code":-32601,"message":PRIVATE}}),
        ),
        Case::Flood | Case::WireLimit => {
            let payload = if matches!(case, Case::WireLimit) {
                "x".repeat(120 * 1024)
            } else {
                String::new()
            };
            for _ in 0..70 {
                if socket
                    .send(Message::text(
                        json!({"method":"notice","params":payload}).to_string(),
                    ))
                    .is_err()
                {
                    break;
                }
            }
        }
        _ => {
            let status = match case {
                Case::ActiveApproval => {
                    json!({"type":"active","activeFlags":["waitingOnApproval"]})
                }
                Case::UnknownState => json!({"type":PRIVATE}),
                Case::UnknownFlag => json!({"type":"active","activeFlags":[PRIVATE]}),
                _ => json!({"type":"idle"}),
            };
            send(
                &mut socket,
                json!({"method":"thread/status/changed","params":{"private":PRIVATE}}),
            );
            send(
                &mut socket,
                json!({
                    "id":if matches!(case, Case::WrongCorrelation) { json!("foreign-request") } else { read["id"].clone() },
                    "result":{"thread":{
                        "id":if matches!(case, Case::WrongThread) { "foreign-thread" } else { "thread-exact" },
                        "status":status, "preview":PRIVATE, "turns":[{"private":PRIVATE}]
                    }}
                }),
            );
        }
    }
    drain(&mut socket, &mut log);
    log
}
fn fixture(case: Case, connections: usize) -> (TempDir, thread::JoinHandle<Vec<Value>>) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("server.sock");
    let listener = UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    // A broken client must not leave the fixture waiting indefinitely in accept.
    listener.set_nonblocking(true).unwrap();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut log = Vec::new();
        let mut accepted = 0;
        while accepted < connections {
            match listener.accept() {
                Ok((stream, _)) => {
                    log.extend(serve(stream, case));
                    accepted += 1;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "fixture never connected");
                    thread::sleep(Duration::from_millis(2));
                }
                Err(error) => panic!("{error}"),
            }
        }
        log
    });
    (root, handle)
}
fn run(root: &Path, timeout_ms: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_reignite"))
        .args(["--state-dir"])
        .arg(root.join("recovery-state"))
        .args(["probe-codex", "--socket"])
        .arg(root.join("server.sock"))
        .args([
            "--thread",
            "thread-exact",
            "--protocol",
            "app-server-v2",
            "--timeout-ms",
            timeout_ms,
        ])
        .output()
        .unwrap()
}
fn value(output: &Output) -> Value {
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        !text.contains(PRIVATE),
        "private server text leaked: {text}"
    );
    serde_json::from_str(&text).unwrap()
}
fn assert_read_only(log: &[Value]) {
    for request in log {
        assert!(
            matches!(
                request["method"].as_str(),
                Some("initialize" | "initialized" | "thread/read")
            ),
            "unexpected request/answer: {request}"
        );
    }
}
#[test]
fn matching_read_and_reconnect_never_load_or_start_work() {
    let (root, server) = fixture(Case::Good, 2);
    for _ in 0..2 {
        let output = run(root.path(), "1000");
        let value = value(&output);
        assert!(output.status.success(), "{value}");
        let report = &value["result"];
        assert_eq!(report["handshake_verified"], true);
        assert_eq!(report["peer_uid_verified"], true);
        assert_eq!(report["thread_id"], "thread-exact");
        assert_eq!(report["reported_state"], "idle");
        assert_eq!(report["execution_owner_verified"], false);
        assert_eq!(report["eligible"], false);
        assert_eq!(report["pending_decision_reported"], Value::Null);
        assert_eq!(report["notifications_seen"], 1);
        assert!(!root.path().join("recovery-state").exists());
    }
    let log = server.join().unwrap();
    assert_read_only(&log);
    assert_eq!(log.len(), 6);
}
#[test]
fn partial_and_approval_states_never_authorize_recovery() {
    for case in [Case::ActiveApproval, Case::UnknownState, Case::UnknownFlag] {
        let (root, server) = fixture(case, 1);
        let output = run(root.path(), "1000");
        let value = value(&output);
        assert!(output.status.success(), "{case:?}: {value}");
        assert_eq!(value["result"]["eligible"], false);
        if matches!(case, Case::ActiveApproval) {
            assert_eq!(value["result"]["pending_decision_reported"], true);
        } else {
            assert_eq!(value["result"]["pending_decision_reported"], Value::Null);
        }
        assert_read_only(&server.join().unwrap());
    }
}
#[test]
fn invalid_replies_disconnects_and_requests_fail_without_answer_or_state() {
    for case in [
        Case::WrongThread,
        Case::WrongCorrelation,
        Case::Corrupt,
        Case::Oversized,
        Case::InitRejected,
        Case::ReadRejected,
        Case::BadInit,
        Case::ApprovalRequest,
        Case::Disconnect,
        Case::Flood,
        Case::WireLimit,
    ] {
        let (root, server) = fixture(case, 1);
        let output = run(root.path(), "1000");
        assert!(!output.status.success(), "{case:?}");
        assert_eq!(value(&output)["ok"], false);
        assert!(!root.path().join("recovery-state").exists());
        assert_read_only(&server.join().unwrap());
    }
}
#[test]
fn overall_deadline_also_bounds_a_trickling_peer() {
    for case in [Case::Timeout, Case::Trickle] {
        let (root, server) = fixture(case, 1);
        let started = Instant::now();
        let output = run(root.path(), "100");
        assert!(!output.status.success());
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(value(&output)["ok"], false);
        assert_read_only(&server.join().unwrap());
    }
}
#[test]
fn a_non_websocket_service_or_oversized_http_header_never_receives_rpc() {
    for oversized in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("server.sock");
        let listener = UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0; 4096];
            let count = stream.read(&mut request).unwrap();
            assert!(request[..count].starts_with(b"GET / HTTP/1.1"));
            let response = if oversized {
                format!(
                    "HTTP/1.1 101 Switching Protocols\r\nX-Padding: {}\r\n\r\n",
                    "x".repeat(2 * 1024 * 1024)
                )
            } else {
                format!(
                    "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\n\r\n{PRIVATE}",
                    PRIVATE.len()
                )
            };
            let _ = stream.write_all(response.as_bytes());
            let mut remainder = Vec::new();
            let _ = stream.take(4096).read_to_end(&mut remainder);
            let received = [request[..count].to_vec(), remainder].concat();
            assert!(!String::from_utf8_lossy(&received).contains("\"method\""));
        });
        let output = run(root.path(), "1000");
        assert!(!output.status.success());
        assert_eq!(value(&output)["ok"], false);
        server.join().unwrap();
    }
}
#[test]
fn unsafe_endpoints_and_unsupported_profiles_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("server.sock");
    fs::write(&path, "not a socket").unwrap();
    assert!(!run(root.path(), "100").status.success());
    fs::remove_file(&path).unwrap();
    let _listener = UnixListener::bind(root.path().join("real.sock")).unwrap();
    std::os::unix::fs::symlink(root.path().join("real.sock"), &path).unwrap();
    assert!(!run(root.path(), "100").status.success());
    fs::remove_file(&path).unwrap();
    let _listener2 = UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
    assert!(!run(root.path(), "100").status.success());
    let output = Command::new(env!("CARGO_BIN_EXE_reignite"))
        .args(["probe-codex", "--socket"])
        .arg(&path)
        .args(["--thread", "thread-exact", "--protocol", "remote-control"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!root.path().join("recovery-state").exists());
}
