//! A bounded, inspection-only Codex app-server v2 client over a same-user Unix socket.
//! No loading, subscriptions, execution, approval answers, retries, or recovery state.
use anyhow::{Result, anyhow, bail, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use socket2::{Domain, SockAddr, Socket, Type};
use std::{
    fs,
    io::{self, Read, Write},
    mem,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileTypeExt, MetadataExt},
            net::UnixStream,
        },
    },
    path::Path,
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, client::client_with_config, protocol::WebSocketConfig};

const MAX_MESSAGE: usize = 128 * 1024;
const MAX_WIRE_BYTES: usize = 1024 * 1024;
const MAX_MESSAGES: usize = 64;

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: u32,
    pub harness: &'static str,
    pub protocol: &'static str,
    pub transport: &'static str,
    pub thread_id: String,
    pub handshake_verified: bool,
    pub peer_uid_verified: bool,
    pub reported_state: Option<String>,
    pub pending_decision_reported: Option<bool>,
    pub execution_owner_verified: bool,
    pub eligible: bool,
    pub missing_evidence: Vec<&'static str>,
    pub warnings: Vec<&'static str>,
    pub notifications_seen: usize,
    pub bytes_read: usize,
    pub elapsed_micros: u128,
}

// The deadline and byte limit include the HTTP upgrade, fragmented frames and
// ignored notifications. Per-operation timeouts alone permit unbounded trickles.
struct BoundedStream {
    stream: UnixStream,
    deadline: Instant,
    bytes_read: usize,
}
impl BoundedStream {
    fn remaining(&self) -> io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "probe deadline exceeded"))
    }
}
impl Read for BoundedStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let remaining = MAX_WIRE_BYTES - self.bytes_read;
        if remaining == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "probe wire limit exceeded",
            ));
        }
        self.stream.set_read_timeout(Some(self.remaining()?))?;
        let length = buffer.len().min(remaining);
        let read = self.stream.read(&mut buffer[..length])?;
        self.bytes_read += read;
        Ok(read)
    }
}
impl Write for BoundedStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.stream.set_write_timeout(Some(self.remaining()?))?;
        self.stream.write(buffer)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stream.set_write_timeout(Some(self.remaining()?))?;
        self.stream.flush()
    }
}

struct Client {
    socket: WebSocket<BoundedStream>,
    messages_seen: usize,
    notifications_seen: usize,
}
impl Client {
    fn send(&mut self, message: Value) -> Result<()> {
        self.socket
            .send(Message::text(message.to_string()))
            .map_err(|_| anyhow!("probe write failed or timed out"))
    }
    fn response(&mut self, id: &str) -> Result<Value> {
        loop {
            ensure!(
                self.messages_seen < MAX_MESSAGES,
                "probe message limit exceeded"
            );
            self.messages_seen += 1;
            let message = self.socket.read().map_err(|error| match error {
                tungstenite::Error::Capacity(_) => anyhow!("oversized WebSocket reply"),
                tungstenite::Error::Io(error) if error.kind() == io::ErrorKind::InvalidData => {
                    anyhow!("probe wire limit exceeded")
                }
                _ => anyhow!("probe read failed, disconnected, or timed out"),
            })?;
            let text = match message {
                Message::Text(text) => text,
                Message::Ping(_) | Message::Pong(_) => continue,
                Message::Close(_) => bail!("probe disconnected before response"),
                _ => bail!("unsupported WebSocket message"),
            };
            let value: Value =
                serde_json::from_str(&text).map_err(|_| anyhow!("malformed JSON reply"))?;
            ensure!(value.is_object(), "invalid response envelope");
            if let Some(version) = value.get("jsonrpc") {
                ensure!(version == "2.0", "unsupported RPC version");
            }
            if value.get("method").is_some() {
                // Never answer a server request, including human approvals.
                ensure!(
                    value.get("id").is_none(),
                    "unexpected server request; no answer sent"
                );
                ensure!(
                    value["method"].is_string()
                        && value.get("result").is_none()
                        && value.get("error").is_none(),
                    "invalid notification envelope"
                );
                self.notifications_seen += 1;
                continue;
            }
            ensure!(
                value["id"].as_str() == Some(id),
                "response correlation mismatch"
            );
            ensure!(
                value.get("result").is_some() != value.get("error").is_some(),
                "invalid response envelope"
            );
            if let Some(error) = value.get("error") {
                // Do not echo server error messages: they can contain private text.
                let code = error["code"].as_i64();
                bail!("server rejected {id} (code {code:?})");
            }
            return Ok(value["result"].clone());
        }
    }
}

fn verify_peer(stream: &UnixStream) -> Result<()> {
    // Linux SO_PEERCRED authenticates the connected process, not thread ownership.
    let mut credentials: libc::ucred = unsafe { mem::zeroed() };
    let mut length = mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    ensure!(
        result == 0
            && length as usize == mem::size_of::<libc::ucred>()
            && credentials.uid == unsafe { libc::geteuid() },
        "socket peer is not the current user"
    );
    Ok(())
}

pub fn probe(path: &Path, thread_id: &str, timeout: Duration) -> Result<Report> {
    let started = Instant::now();
    ensure!(
        !timeout.is_zero() && timeout <= Duration::from_secs(30),
        "invalid probe timeout"
    );
    ensure!(
        !thread_id.is_empty()
            && thread_id.len() <= 128
            && thread_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte)),
        "invalid exact thread ID"
    );
    ensure!(
        path.is_absolute() && fs::canonicalize(path)? == path,
        "socket path must be absolute and canonical"
    );
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.file_type().is_socket()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o022 == 0,
        "socket must be user-owned and not group/other writable"
    );
    let socket = Socket::new(Domain::UNIX, Type::STREAM, None)?;
    let remaining = timeout
        .checked_sub(started.elapsed())
        .ok_or_else(|| anyhow!("probe timed out"))?;
    socket
        .connect_timeout(&SockAddr::unix(path)?, remaining)
        .map_err(|_| anyhow!("socket connection failed or timed out"))?;
    let stream = UnixStream::from(std::os::fd::OwnedFd::from(socket));
    verify_peer(&stream)?;
    let stream = BoundedStream {
        stream,
        deadline: started + timeout,
        bytes_read: 0,
    };
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE))
        .max_frame_size(Some(MAX_MESSAGE));
    // This URL supplies HTTP upgrade headers only. No TCP connection is made.
    let (socket, _) = client_with_config("ws://localhost/", stream, Some(config))
        .map_err(|_| anyhow!("WebSocket handshake failed or timed out; protocol unverified"))?;
    let mut client = Client {
        socket,
        messages_seen: 0,
        notifications_seen: 0,
    };
    client.send(json!({"id":"initialize", "method":"initialize", "params":{
        "clientInfo":{"name":"reignite-probe", "title":"Reignite read-only probe", "version":env!("CARGO_PKG_VERSION")},
        "capabilities":{"experimentalApi":false}
    }}))?;
    let initialized = client.response("initialize")?;
    ensure!(
        initialized["userAgent"]
            .as_str()
            .is_some_and(|agent| !agent.is_empty()),
        "unsupported initialization result"
    );
    client.send(json!({"method":"initialized"}))?;
    client.send(
        json!({"id":"thread-read", "method":"thread/read", "params":{
            "threadId":thread_id, "includeTurns":false
        }}),
    )?;
    let read = client.response("thread-read")?;
    ensure!(
        read["thread"]["id"].as_str() == Some(thread_id),
        "thread identity mismatch"
    );
    let status = &read["thread"]["status"];
    let reported_state = status["type"]
        .as_str()
        .filter(|state| matches!(*state, "notLoaded" | "idle" | "systemError" | "active"))
        .map(str::to_owned);
    let pending_decision_reported = (reported_state.as_deref() == Some("active"))
        .then(|| status["activeFlags"].as_array())
        .flatten()
        .and_then(|flags| {
            flags
                .iter()
                .all(|flag| {
                    matches!(
                        flag.as_str(),
                        Some("waitingOnApproval" | "waitingOnUserInput")
                    )
                })
                .then_some(!flags.is_empty())
        });
    let mut warnings = Vec::new();
    if reported_state.is_none() {
        warnings.push("thread_status_unavailable_or_unsupported");
    }
    if pending_decision_reported.is_none() {
        warnings.push("pending_decision_state_unavailable");
    }
    Ok(Report {
        schema: 1,
        harness: "codex",
        protocol: "app-server-v2",
        transport: "unix-websocket",
        thread_id: thread_id.to_owned(),
        handshake_verified: true,
        peer_uid_verified: true,
        reported_state,
        pending_decision_reported,
        execution_owner_verified: false,
        eligible: false,
        missing_evidence: vec![
            "cross_server_execution_ownership",
            "crash_time_user_intent",
            "restore_authorization_and_attempt_state",
        ],
        warnings,
        notifications_seen: client.notifications_seen,
        bytes_read: client.socket.get_ref().bytes_read,
        elapsed_micros: started.elapsed().as_micros(),
    })
}
