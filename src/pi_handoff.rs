//! Explicit operator handoff to Pi's ordinary interactive client.
//! No automatic recovery, prompt injection, RPC-to-TUI attachment or new UI.
use crate::{Request, Store, inspection, pi_rpc::validate_profile};
use anyhow::{Result, bail, ensure};
use std::{
    ffi::OsString,
    io::{self, IsTerminal, Write},
    os::unix::process::CommandExt,
    path::Path,
    process::Command,
};

pub fn handoff(store: &Store, session: &Path, program: &Path, profile: &[OsString]) -> Result<()> {
    ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal() && io::stderr().is_terminal(),
        "Pi operator handoff requires an interactive terminal on stdin/stdout/stderr"
    );
    validate_profile(program, profile)?;
    // Complete bounded inspection before invoking a loader that could repair input.
    let source = inspection::inspect(session, None)?.session;
    let policy = store.execute(Request::PolicyStatus {
        session_file: Some(session.to_owned()),
    })?;
    ensure!(
        policy["session"]["session_id"].as_str() == Some(&source.native_id)
            && policy["session"]["cwd"].as_str() == source.cwd.to_str(),
        "handoff source identity changed"
    );
    let snapshot = store.execute(Request::Status)?;
    let boot = crate::boot_id()?;
    for value in snapshot["records"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("missing retained records"))?
    {
        let record: crate::Record = serde_json::from_value(value.clone())?;
        if record.identity.harness == "pi"
            && record.identity.session_id == source.native_id
            && crate::live(&record.owner, &boot)
        {
            bail!("known retained local owner is live; use its existing operator client");
        }
    }
    // Disable controls recovery, not ordinary explicit operator use. Never clear
    // it, claim/reset a ticket or manufacture a lifecycle observation here.
    let mut terminal = io::stderr().lock();
    writeln!(
        terminal,
        "Reignite: manual Pi handoff only; no continuation or approval is being sent."
    )?;
    writeln!(
        terminal,
        "Saved history does not prove the lost active branch, waits, or exclusive ownership."
    )?;
    writeln!(
        terminal,
        "Use the existing client if another owner survives. Review branch/intent in Pi before work."
    )?;
    if policy["policy_enabled_for_session"].as_bool() == Some(false) {
        writeln!(
            terminal,
            "Recovery is disabled; that policy and all retained attempts remain unchanged."
        )?;
    }
    terminal.flush()?;
    drop(terminal);
    // No deadline/EOF disposal: the real operator client stays attached, handles
    // dialogs and owns its native lifecycle. PID is preserved across exec.
    let error = Command::new(program)
        .args(profile)
        .arg("--session")
        .arg(session)
        .current_dir(&source.cwd)
        .exec();
    bail!("Pi interactive handoff failed ({:?})", error.kind())
}
