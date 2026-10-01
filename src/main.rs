use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use reignite::{Request, Store};
use serde_json::json;
use std::{
    io::{self, Read},
    path::PathBuf,
};

#[derive(Parser)]
#[command(version, about = "Standalone Linux agent recovery tools")]
struct Cli {
    #[arg(long, env = "REIGNITE_STATE_DIR")]
    state_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Find saved Pi sessions and explain what prevents recovery. Never start work.
    Discover {
        /// Existing custom session directories; repeat for more than one location.
        #[arg(long = "sessions-root")]
        sessions_roots: Vec<PathBuf>,
        /// Machine-readable output instead of the plain-English report.
        #[arg(long)]
        json: bool,
    },
    /// Read native Pi history and optional pi-subagents status, without changing state.
    Inspect {
        #[arg(long)]
        session: PathBuf,
        /// Existing PI_SUBAGENTS_TEMP_ROOT; async-subagent-runs is read beneath it.
        #[arg(long, env = "PI_SUBAGENTS_TEMP_ROOT")]
        subagents_root: Option<PathBuf>,
    },
    /// Assess native Pi evidence and policy; never load, authorize or start work.
    AssessPi {
        #[arg(long)]
        session: PathBuf,
        #[arg(long, env = "PI_SUBAGENTS_TEMP_ROOT")]
        subagents_root: Option<PathBuf>,
        /// Optional existing Fabric Pi usage JSONL; negative evidence only.
        #[arg(long)]
        usage_log: Option<PathBuf>,
    },
    /// Inspect one Codex thread through a configured same-user Unix WebSocket.
    ProbeCodex {
        #[arg(long)]
        socket: PathBuf,
        #[arg(long)]
        thread: String,
        #[arg(long, value_parser = ["app-server-v2"])]
        protocol: String,
        #[arg(long, default_value_t = 5000, value_parser = clap::value_parser!(u64).range(1..=30_000))]
        timeout_ms: u64,
    },
    /// Manual operator handoff to Pi's real terminal UI; no automatic continuation.
    HandoffPi {
        #[arg(long)]
        session: PathBuf,
        #[arg(long)]
        pi_program: PathBuf,
        #[arg(last = true)]
        profile: Vec<std::ffi::OsString>,
    },
    /// Experimental owned-Pi RPC transport using an existing legacy restore ticket.
    DeliverPi {
        #[arg(long)]
        session: PathBuf,
        #[arg(long)]
        ticket: String,
        #[arg(long)]
        pi_program: PathBuf,
        #[arg(long, default_value_t = 30_000, value_parser = clap::value_parser!(u64).range(1..=120_000))]
        timeout_ms: u64,
        /// Explicit profile options only; no saved commands, prompts or session overrides.
        #[arg(last = true)]
        profile: Vec<std::ffi::OsString>,
    },
    /// Host-wide recovery policy and durable native-session overrides.
    Policy {
        #[command(subcommand)]
        command: PolicyCommand,
    },
    /// Adapter JSON request on stdin (schema documented in docs/cli-reference.md).
    Request,
    Status,
    Disable {
        #[arg(long)]
        session: PathBuf,
    },
    /// Authorize one legacy 120-second restore ticket, or inspect eligibility.
    Recover {
        #[arg(long)]
        session: PathBuf,
        #[arg(long)]
        dry_run: bool,
    },
}
#[derive(Subcommand)]
enum PolicyCommand {
    Show {
        #[arg(long)]
        session: Option<PathBuf>,
    },
    Enable,
    Disable,
    /// Remove a session override, without resetting attempts or enrolling a session.
    ClearDisable {
        #[arg(long)]
        session: PathBuf,
    },
}
enum Output {
    Json(serde_json::Value),
    Text(String),
}
fn run(cli: Cli) -> Result<Output> {
    let dir = cli.state_dir.unwrap_or_else(|| {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
            })
            .join("reignite")
    });
    let req = match cli.command {
        Command::Discover {
            sessions_roots,
            json,
        } => {
            let value = Store::new(dir)?.execute(Request::DiscoverPi { sessions_roots })?;
            return if json {
                Ok(Output::Json(value))
            } else {
                let report: reignite::discovery::Discovery = serde_json::from_value(value)?;
                Ok(Output::Text(report.plain_text()))
            };
        }
        Command::Inspect {
            session,
            subagents_root,
        } => {
            return Ok(Output::Json(serde_json::to_value(
                reignite::inspection::inspect(&session, subagents_root.as_deref())?,
            )?));
        }
        Command::AssessPi {
            session,
            subagents_root,
            usage_log,
        } => Request::AssessPi {
            session_file: session,
            subagents_root,
            usage_log,
        },
        Command::ProbeCodex {
            socket,
            thread,
            protocol: _,
            timeout_ms,
        } => {
            return Ok(Output::Json(serde_json::to_value(
                reignite::codex_probe::probe(
                    &socket,
                    &thread,
                    std::time::Duration::from_millis(timeout_ms),
                )?,
            )?));
        }
        Command::HandoffPi {
            session,
            pi_program,
            profile,
        } => {
            reignite::pi_handoff::handoff(&Store::new(dir)?, &session, &pi_program, &profile)?;
            anyhow::bail!("Pi handoff returned unexpectedly");
        }
        Command::DeliverPi {
            session,
            ticket,
            pi_program,
            timeout_ms,
            profile,
        } => {
            return Ok(Output::Json(serde_json::to_value(
                reignite::pi_delivery::deliver(
                    &Store::new(dir)?,
                    &session,
                    &ticket,
                    &pi_program,
                    &profile,
                    std::time::Duration::from_millis(timeout_ms),
                )?,
            )?));
        }
        Command::Policy { command } => match command {
            PolicyCommand::Show { session } => Request::PolicyStatus {
                session_file: session,
            },
            PolicyCommand::Enable => Request::SetHostPolicy { enabled: true },
            PolicyCommand::Disable => Request::SetHostPolicy { enabled: false },
            PolicyCommand::ClearDisable { session } => Request::ClearDisable {
                session_file: session,
            },
        },
        Command::Request => {
            let mut text = String::new();
            io::stdin().take(64 * 1024 + 1).read_to_string(&mut text)?;
            anyhow::ensure!(text.len() <= 64 * 1024, "request too large");
            serde_json::from_str(&text).context("invalid protocol request")?
        }
        Command::Status => Request::Status,
        Command::Disable { session } => Request::Disable {
            session_file: session,
        },
        Command::Recover { session, dry_run } => Request::Recover {
            session_file: session,
            dry_run,
        },
    };
    Store::new(dir)?.execute(req).map(Output::Json)
}
fn main() {
    let cli = Cli::parse();
    let plain = matches!(&cli.command, Command::Discover { json: false, .. });
    match run(cli) {
        Ok(Output::Json(value)) => println!("{}", json!({"ok":true,"result":value})),
        Ok(Output::Text(text)) => print!("{text}"),
        Err(error) => {
            if plain {
                let message: String = format!("{error:#}")
                    .chars()
                    .flat_map(char::escape_default)
                    .collect();
                eprintln!("Could not check saved sessions: {message}");
            } else {
                println!("{}", json!({"ok":false,"error":format!("{error:#}")}));
            }
            std::process::exit(1);
        }
    }
}
