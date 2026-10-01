//! Bounded JSONL I/O for an explicitly owned Pi subprocess. No server or attachment.
use anyhow::{Result, anyhow, bail, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, VecDeque},
    ffi::OsString,
    fs,
    io::{self, Read, Write},
    os::{
        fd::AsRawFd,
        unix::{fs::MetadataExt, process::CommandExt},
    },
    path::Path,
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};

const MAX_RECORD: usize = 1024 * 1024;
const MAX_STDOUT: usize = 8 * 1024 * 1024;
const MAX_STDERR: usize = 256 * 1024;
const MAX_EVENTS: usize = 8192;

pub(crate) fn validate_profile(program: &Path, args: &[OsString]) -> Result<()> {
    crate::canonical(program)?;
    let metadata = fs::metadata(program)?;
    ensure!(
        metadata.is_file()
            && metadata.mode() & 0o111 != 0
            && metadata.mode() & 0o022 == 0
            && (metadata.uid() == 0 || metadata.uid() == unsafe { libc::geteuid() }),
        "unsafe Pi executable"
    );
    ensure!(
        args.len() <= 64 && args.iter().map(|arg| arg.len()).sum::<usize>() <= 32 * 1024,
        "Pi profile argument limit exceeded"
    );
    let mut index = 0;
    while index < args.len() {
        let flag = args[index]
            .to_str()
            .ok_or_else(|| anyhow!("non-UTF8 profile argument"))?;
        let value = match flag {
            "--provider"
            | "--model"
            | "--thinking"
            | "--models"
            | "--api-key"
            | "--tools"
            | "-t"
            | "--exclude-tools"
            | "-xt"
            | "--extension"
            | "-e"
            | "--skill"
            | "--prompt-template"
            | "--system-prompt"
            | "--append-system-prompt" => true,
            "--offline"
            | "--no-tools"
            | "-nt"
            | "--no-builtin-tools"
            | "-nbt"
            | "--no-extensions"
            | "-ne"
            | "--no-skills"
            | "-ns"
            | "--no-prompt-templates"
            | "-np"
            | "--no-context-files"
            | "-nc"
            | "--no-approve"
            | "-na"
            | "--approve"
            | "-a" => false,
            _ => bail!(
                "unsupported Pi profile argument; prompts/session/mode overrides are forbidden"
            ),
        };
        index += 1;
        if value {
            ensure!(index < args.len(), "missing Pi profile value");
            let value = args[index]
                .to_str()
                .ok_or_else(|| anyhow!("non-UTF8 profile value"))?;
            ensure!(
                !value.is_empty() && !value.starts_with('-'),
                "invalid Pi profile value"
            );
            index += 1;
        }
    }
    Ok(())
}
fn nonblocking(fd: i32) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    ensure!(
        flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == 0,
        "cannot configure Pi pipe"
    );
    Ok(())
}

pub(crate) struct Client {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    errors: ChildStderr,
    output_open: bool,
    errors_open: bool,
    buffer: Vec<u8>,
    responses: VecDeque<Value>,
    next_id: u64,
    expected: Option<(String, String)>,
    deadline: Instant,
    pub stdout_bytes: usize,
    stderr_bytes: usize,
    pub events: usize,
    pub dialogs: BTreeSet<String>,
    pub activity_seen: bool,
    pub settled: bool,
    pub failure_seen: bool,
}
impl Client {
    pub fn start(
        program: &Path,
        args: &[OsString],
        session: &Path,
        cwd: &Path,
        timeout: Duration,
    ) -> Result<Self> {
        validate_profile(program, args)?;
        ensure!(
            !timeout.is_zero() && timeout <= Duration::from_secs(120),
            "invalid Pi timeout"
        );
        let deadline = Instant::now() + timeout;
        let parent = unsafe { libc::getpid() };
        let mut command = Command::new(program);
        command
            .args(args)
            .arg("--mode")
            .arg("rpc")
            .arg("--session")
            .arg(session)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Own only this child. Never signal a process group containing harness children.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::getppid() != parent {
                    libc::_exit(1);
                }
                Ok(())
            });
        }
        let mut child = command
            .spawn()
            .map_err(|error| anyhow!("Pi subprocess failed to start ({:?})", error.kind()))?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("missing Pi stdin"))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("missing Pi stdout"))?;
        let errors = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("missing Pi stderr"))?;
        if nonblocking(input.as_raw_fd())
            .and_then(|_| nonblocking(output.as_raw_fd()))
            .and_then(|_| nonblocking(errors.as_raw_fd()))
            .is_err()
        {
            drop(input);
            drop(output);
            drop(errors);
            let _ = child.kill();
            let _ = child.wait();
            bail!("cannot configure Pi pipes");
        }
        let client = Self {
            child,
            input: Some(input),
            output,
            errors,
            output_open: true,
            errors_open: true,
            buffer: Vec::new(),
            responses: VecDeque::new(),
            next_id: 0,
            expected: None,
            deadline,
            stdout_bytes: 0,
            stderr_bytes: 0,
            events: 0,
            dialogs: BTreeSet::new(),
            activity_seen: false,
            settled: false,
            failure_seen: false,
        };
        Ok(client)
    }
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
    fn remaining(&self) -> Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|time| !time.is_zero())
            .ok_or_else(|| anyhow!("Pi RPC deadline exceeded"))
    }
    fn poll(&self, input: bool) -> Result<()> {
        let duration = self.remaining()?;
        let mut fds = [
            libc::pollfd {
                fd: if self.output_open {
                    self.output.as_raw_fd()
                } else {
                    -1
                },
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: if self.errors_open {
                    self.errors.as_raw_fd()
                } else {
                    -1
                },
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: if input {
                    self.input
                        .as_ref()
                        .ok_or_else(|| anyhow!("Pi stdin closed"))?
                        .as_raw_fd()
                } else {
                    -1
                },
                events: libc::POLLOUT,
                revents: 0,
            },
        ];
        let milliseconds = duration.as_millis().clamp(1, i32::MAX as u128) as i32;
        let result =
            unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, milliseconds) };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                return Ok(());
            }
            bail!("Pi pipe polling failed");
        }
        ensure!(result != 0, "Pi RPC deadline exceeded");
        Ok(())
    }
    fn record(&mut self, line: &[u8]) -> Result<()> {
        ensure!(line.len() <= MAX_RECORD, "Pi record limit exceeded");
        self.events += 1;
        ensure!(self.events <= MAX_EVENTS, "Pi event limit exceeded");
        let value: Value =
            serde_json::from_slice(line).map_err(|_| anyhow!("malformed Pi JSONL"))?;
        ensure!(value.is_object(), "invalid Pi event envelope");
        let kind = value["type"]
            .as_str()
            .ok_or_else(|| anyhow!("Pi event has no type"))?;
        match kind {
            "response" => {
                ensure!(self.responses.len() < 8, "Pi response queue limit exceeded");
                self.responses.push_back(value);
            }
            "extension_ui_request" => {
                ensure!(value["id"].is_string(), "invalid Pi UI request");
                match value["method"].as_str() {
                    Some(method @ ("select" | "confirm" | "input" | "editor")) => {
                        self.dialogs.insert(method.to_owned());
                    }
                    Some("notify" | "setStatus" | "setWidget" | "setTitle" | "set_editor_text") => {
                    }
                    _ => bail!("unsupported Pi UI request; no answer sent"),
                }
            }
            "agent_start" | "turn_start" | "tool_execution_start" => {
                self.activity_seen = true;
                self.settled = false;
            }
            "agent_settled" => self.settled = true,
            "extension_error" => self.failure_seen = true,
            "message_end" => {
                if matches!(
                    value["message"]["stopReason"].as_str(),
                    Some("error" | "aborted")
                ) {
                    self.failure_seen = true;
                }
            }
            "auto_retry_start" | "summarization_retry_attempt_start" => {
                self.activity_seen = true;
                self.settled = false;
            }
            "message_start"
            | "message_update"
            | "turn_end"
            | "agent_end"
            | "tool_execution_update"
            | "tool_execution_end"
            | "queue_update"
            | "entry_appended"
            | "session_info_changed"
            | "thinking_level_changed"
            | "compaction_start"
            | "compaction_end"
            | "auto_retry_end"
            | "summarization_retry_scheduled"
            | "summarization_retry_finished" => {}
            _ => bail!("unsupported Pi event; continuation held"),
        }
        Ok(())
    }
    fn drain(&mut self) -> Result<()> {
        // Alternate streams so a stderr flood cannot starve stdout or the deadline.
        for _ in 0..32 {
            self.remaining()?;
            let mut progress = false;
            let mut chunk = [0; 8192];
            if self.output_open {
                match self.output.read(&mut chunk) {
                    Ok(0) => {
                        self.output_open = false;
                        ensure!(self.buffer.is_empty(), "incomplete Pi JSONL write");
                    }
                    Ok(length) => {
                        progress = true;
                        self.stdout_bytes += length;
                        ensure!(self.stdout_bytes <= MAX_STDOUT, "Pi stdout limit exceeded");
                        self.buffer.extend_from_slice(&chunk[..length]);
                        while let Some(end) = self.buffer.iter().position(|byte| *byte == b'\n') {
                            let mut line: Vec<u8> = self.buffer.drain(..=end).collect();
                            line.pop();
                            if line.last() == Some(&b'\r') {
                                line.pop();
                            }
                            self.record(&line)?;
                        }
                        ensure!(self.buffer.len() <= MAX_RECORD, "Pi record limit exceeded");
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(_) => bail!("Pi stdout read failed"),
                }
            }
            if self.errors_open {
                match self.errors.read(&mut chunk) {
                    Ok(0) => self.errors_open = false,
                    Ok(length) => {
                        progress = true;
                        self.stderr_bytes += length;
                        ensure!(self.stderr_bytes <= MAX_STDERR, "Pi stderr limit exceeded");
                        // Diagnostics can contain secrets; drain, never emit or persist.
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(_) => bail!("Pi stderr read failed"),
                }
            }
            if !progress {
                break;
            }
        }
        Ok(())
    }
    pub fn send(&mut self, kind: &str, params: Value) -> Result<()> {
        ensure!(
            matches!(
                kind,
                "get_state" | "get_entries" | "get_messages" | "prompt"
            ),
            "unsupported Pi command"
        );
        let mut value = params;
        ensure!(value.is_object(), "invalid Pi parameters");
        ensure!(self.expected.is_none(), "Pi command already pending");
        self.next_id += 1;
        let id = format!("reignite-{}", self.next_id);
        value["id"] = json!(id);
        value["type"] = json!(kind);
        self.expected = Some((id, kind.to_owned()));
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        ensure!(bytes.len() <= MAX_RECORD, "Pi command limit exceeded");
        let mut written = 0;
        while written < bytes.len() {
            self.remaining()?;
            self.drain()?;
            if kind == "prompt" {
                ensure!(
                    self.dialogs.is_empty() && !self.failure_seen && !self.activity_seen,
                    "Pi decision/activity appeared before prompt write; claim retained"
                );
            }
            match self
                .input
                .as_mut()
                .ok_or_else(|| anyhow!("Pi stdin closed"))?
                .write(&bytes[written..])
            {
                Ok(0) => bail!("Pi stdin disconnected"),
                Ok(length) => written += length,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => self.poll(true)?,
                Err(_) => bail!("Pi stdin write failed; delivery may be uncertain"),
            }
        }
        Ok(())
    }
    pub fn response(&mut self, kind: &str) -> Result<Option<Value>> {
        loop {
            self.drain()?;
            if let Some(value) = self.responses.pop_front() {
                ensure!(
                    self.expected
                        .as_ref()
                        .is_some_and(|(id, command)| value["id"] == *id
                            && value["command"] == *command
                            && command == kind),
                    "Pi response correlation mismatch"
                );
                ensure!(
                    value["success"].as_bool() == Some(true),
                    "Pi command rejected; server error text withheld"
                );
                self.expected.take();
                return Ok(Some(value["data"].clone()));
            }
            if !self.dialogs.is_empty() || self.failure_seen {
                return Ok(None);
            }
            ensure!(self.output_open, "Pi disconnected before response");
            self.poll(false)?;
        }
    }
    pub fn query(&mut self, kind: &str, params: Value) -> Result<Option<Value>> {
        self.send(kind, params)?;
        self.response(kind)
    }
    pub fn until_settled(&mut self) -> Result<()> {
        loop {
            self.drain()?;
            if self.settled || !self.dialogs.is_empty() || self.failure_seen {
                return Ok(());
            }
            ensure!(self.output_open, "Pi disconnected before agent_settled");
            ensure!(self.responses.is_empty(), "unexpected Pi response");
            self.poll(false)?;
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.input.take(); // Native EOF disposal; never manufacture a UI response.
        let until = Instant::now() + Duration::from_millis(500);
        while Instant::now() < until {
            if self.child.try_wait().is_ok_and(|status| status.is_some()) {
                return;
            }
            // Keep pipes flowing during disposal without parsing/persisting private data.
            let mut bytes = [0; 8192];
            let _ = self.output.read(&mut bytes);
            let _ = self.errors.read(&mut bytes);
            std::thread::sleep(Duration::from_millis(5));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
