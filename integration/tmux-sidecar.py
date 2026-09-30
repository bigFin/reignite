#!/usr/bin/env python3
"""Companion hook: exact Pi reconciliation and bounded restore-ticket delivery.
Run reconcile AFTER upstream save; prepare BEFORE upstream restore. No tmux calls.
"""
import argparse
import json
import os
import pathlib
import subprocess
import tempfile


def cli(*args):
    command = [os.environ.get("REIGNITE_CLI", "reignite")]
    if os.environ.get("REIGNITE_STATE_DIR"):
        command += ["--state-dir", os.environ["REIGNITE_STATE_DIR"]]
    result = subprocess.run(command + list(args), text=True, capture_output=True, check=False)
    response = json.loads(result.stdout)
    if result.returncode or not response["ok"]:
        raise RuntimeError(response.get("error", "recovery CLI failed"))
    return response["result"]


# Deliberately bounded to interactive startup; unknown extension flags are unsafe
# because upstream has lost argv boundaries and cannot infer their arity.
BOOLEAN_OPTIONS = set("--offline --no-extensions -ne --no-skills -ns --no-prompt-templates -np --no-context-files -nc --no-approve -na --approve -a --no-tools -nt --no-builtin-tools -nbt --no-themes --verbose --recovery-interactive".split())
VALUE_OPTIONS = set("--provider --model --thinking --models --tools -t --exclude-tools -xt --extension -e --skill --prompt-template --theme --use-theme --session-dir --name -n --tui-mode --append-system-prompt --system-prompt --recovery-ticket".split())


def safe_interactive_args(args):
    if not isinstance(args, str):
        raise ValueError("cli_args must be text")
    words = args.split()
    index = 0
    while index < len(words):
        word = words[index]
        if word == "--" or word.startswith("@") or not word.startswith("-"):
            raise ValueError("positional prompts/@files/-- tails are not safe to restore; enter tasks interactively")
        option, equal, value = word.partition("=")
        if option in BOOLEAN_OPTIONS and not equal:
            index += 1
            continue
        if option not in VALUE_OPTIONS:
            raise ValueError("unknown/noninteractive option; use documented interactive flags or update the allowlist")
        if equal:
            # The tested Pi CLI parses built-in values as separate argv tokens. Only
            # registered extension flags use its --flag=value parsing branch.
            if option != "--recovery-ticket":
                raise ValueError("Pi built-in options require a separate value, not --option=value")
            if not value:
                raise ValueError("missing option value")
        else:
            index += 1
            if index >= len(words) or words[index].startswith(("-", "@")) or words[index] == "--":
                raise ValueError("missing/ambiguous option value; whitespace in captured values is unsupported")
        index += 1
    return args


def strip_tickets(args):
    # Upstream treats cli_args as whitespace-separated tokens, NOT shell syntax.
    words = args.split()
    output = []
    index = 0
    while index < len(words):
        word = words[index]
        if word == "--recovery-ticket":
            if index + 1 < len(words) and not words[index + 1].startswith("-"):
                index += 1
        elif not word.startswith("--recovery-ticket="):
            output.append(word)
        index += 1
    return " ".join(output) if output != words else args


def live(record, boot):
    owner = record["owner"]
    if owner["boot"] != boot:
        return False
    try:
        fields = pathlib.Path(f'/proc/{owner["pid"]}/stat').read_text().rsplit(")", 1)[1].split()
        return fields[0] != "Z" and fields[19] == owner["start"]
    except (OSError, IndexError):
        return False


def rewrite(path, action):
    path = pathlib.Path(path)
    data = json.loads(path.read_text())
    if not isinstance(data, dict) or not isinstance(data.get("sessions"), list):
        raise ValueError("invalid assistant sidecar")
    status = cli("status")
    records = status["records"]
    sessions = []
    skipped_pi = list(data.get("recovery_skipped_pi", []))
    # New upstream sidecars can also contain session-less command relaunches.
    # They must not bypass the Pi session/argument checks below.
    if "relaunch" in data:
        if not isinstance(data["relaunch"], list):
            raise ValueError("invalid assistant relaunch list")
        relaunch = []
        for original in data["relaunch"]:
            if original.get("tool") == "pi":
                reason = "session-less Pi relaunch is unsupported; reopen manually"
                skipped_pi.append({"entry": original, "reason": reason, "kind": "relaunch"})
                print(f'Pi pane {original.get("pane")}: {reason}', file=__import__("sys").stderr)
            else:
                relaunch.append(original)
        data["relaunch"] = relaunch
    for original in data["sessions"]:
        if original.get("tool") != "pi":
            sessions.append(original)
            continue
        entry = original.copy()
        try:
            entry["cli_args"] = strip_tickets(safe_interactive_args(entry.get("cli_args") or ""))
        except ValueError as error:
            print(f'Pi pane {entry.get("pane")}: skipped unsafe launch: {error}', file=__import__("sys").stderr)
            # Keep the original row available for deliberate manual inspection,
            # outside .sessions so upstream cannot execute it.
            skipped_pi.append({"entry": original, "reason": str(error)})
            continue
        if action == "reconcile":
            matches = [r for r in records if str(r["owner"]["pid"]) == str(entry.get("pid")) and live(r, status["boot"])]
            if len(matches) != 1:
                print(f'Pi pane {entry.get("pane")}: no exact live registration; omitted', file=__import__("sys").stderr)
                continue
            identity = matches[0]["identity"]
            entry["session_id"] = identity["session_id"]
            entry["recovery_session_file"] = identity["session_file"]
            entry["cwd"] = identity["cwd"]
        else:
            # New saves carry both the native ID (for upstream's launcher) and
            # the exact file (for Rust's authorization). Accept old path-based
            # saves too, but never guess from a directory or transcript mtime.
            saved_file = entry.get("recovery_session_file")
            matches = [r for r in records if r["identity"]["cwd"] == entry.get("cwd") and (
                (saved_file is not None and r["identity"]["session_file"] == saved_file and r["identity"]["session_id"] == entry.get("session_id"))
                or (saved_file is None and r["identity"]["session_file"] == entry.get("session_id"))
            )]
            if len(matches) == 1:
                identity = matches[0]["identity"]
                entry["session_id"] = identity["session_id"]
                entry["recovery_session_file"] = identity["session_file"]
                if "--recovery-interactive" in entry["cli_args"].split():
                    try:
                        authorization = cli("recover", "--session", identity["session_file"])
                        entry["cli_args"] += " --recovery-ticket=" + authorization["ticket"]
                    except RuntimeError as error:
                        print(f'Pi pane {entry.get("pane")}: no automatic recovery: {error}', file=__import__("sys").stderr)
            else:
                print(f'Pi pane {entry.get("pane")}: no exact registration; no automatic recovery', file=__import__("sys").stderr)
        sessions.append(entry)
    data["sessions"] = sessions
    if skipped_pi:
        data["recovery_skipped_pi"] = skipped_pi
    # Tickets are not settings; captured env must never become restore authorization.
    fd, temporary = tempfile.mkstemp(prefix=".recovery-", dir=path.parent)
    try:
        with os.fdopen(fd, "w") as output:
            json.dump(data, output, indent=2)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["reconcile", "prepare"])
    parser.add_argument("sidecar")
    arguments = parser.parse_args()
    rewrite(arguments.sidecar, arguments.action)
