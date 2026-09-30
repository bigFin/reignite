use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use reignite::{Request, Store};
use serde_json::json;
use std::{
    io::{self, Read},
    path::PathBuf,
};

#[derive(Parser)]
#[command(version, about = "Opt-in Linux agent recovery (JSON output)")]
struct Cli {
    #[arg(long, env = "REIGNITE_STATE_DIR")]
    state_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Adapter JSON request on stdin (schema documented in README).
    Request,
    Status,
    Disable {
        #[arg(long)]
        session: PathBuf,
    },
    /// Authorize a single 120-second restore ticket, or inspect eligibility.
    Recover {
        #[arg(long)]
        session: PathBuf,
        #[arg(long)]
        dry_run: bool,
    },
}
fn run() -> Result<serde_json::Value> {
    let cli = Cli::parse();
    let dir = cli.state_dir.unwrap_or_else(|| {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
            })
            .join("reignite")
    });
    let req = match cli.command {
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
    Store::new(dir)?.execute(req)
}
fn main() {
    match run() {
        Ok(value) => println!("{}", json!({"ok":true,"result":value})),
        Err(error) => {
            println!("{}", json!({"ok":false,"error":format!("{error:#}")}));
            std::process::exit(1);
        }
    }
}
