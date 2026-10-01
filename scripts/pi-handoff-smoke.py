#!/usr/bin/env python3
"""Actual Pi terminal-client handoff and abrupt PID loss in disposable sessions.
No tmux/Reignite extension, provider credentials, network, deployment or reboot.
"""
import datetime
import fcntl
import json
import os
import pathlib
import pty
import select
import shlex
import shutil
import struct
import subprocess
import tempfile
import termios
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parent.parent
PACKAGE = pathlib.Path(os.environ["PI_PACKAGE_DIR"]).resolve()
metadata = json.loads((PACKAGE / "package.json").read_text())
ENTRY = PACKAGE / metadata["bin"]["pi"]
CLI = (pathlib.Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "debug/reignite").resolve()

with tempfile.TemporaryDirectory(prefix="reignite-pi-handoff-") as temporary:
    base = pathlib.Path(temporary).resolve()
    home, project = base / "home", base / "project with spaces"
    home.mkdir()
    project.mkdir()
    provider, dialogs = base / "provider.ts", base / "dialogs.ts"
    shutil.copyfile(ROOT / "tests/fake-provider.ts", provider)
    shutil.copyfile(ROOT / "tests/pi-handoff-dialog.ts", dialogs)
    launcher = base / "owned-pi"
    launcher.write_text("#!/usr/bin/env bash\nexec node " + shlex.quote(str(ENTRY)) + ' "$@"\n')
    launcher.chmod(0o700)
    calls, events = base / "model-calls", base / "events"
    state_dir = base / "state"
    env = {"PATH": os.environ["PATH"], "HOME": str(home), "TERM": "xterm-256color", "PI_OFFLINE": "1",
           "PI_CODING_AGENT_DIR": str(home / ".pi/agent"),
           "XDG_CONFIG_HOME": str(home / ".config"), "XDG_DATA_HOME": str(home / ".local/share"),
           "XDG_STATE_HOME": str(home / ".local/state"), "XDG_CACHE_HOME": str(home / ".cache"),
           "RECOVERY_SMOKE_OBSERVATIONS": str(calls), "PI_HANDOFF_EVENTS": str(events)}
    session, native_id = base / "session.jsonl", str(uuid.uuid4())
    now = datetime.datetime.now(datetime.timezone.utc).isoformat()
    usage = {key: 0 for key in ("input", "output", "cacheRead", "cacheWrite", "totalTokens")}
    usage["cost"] = {key: 0 for key in ("input", "output", "cacheRead", "cacheWrite", "total")}
    entries = [
        {"type": "session", "version": 3, "id": native_id, "cwd": str(project), "timestamp": now},
        {"type": "model_change", "id": "00000001", "parentId": None, "timestamp": now, "provider": "recovery-smoke", "modelId": "fixture"},
        {"type": "thinking_level_change", "id": "00000002", "parentId": "00000001", "timestamp": now, "thinkingLevel": "off"},
        {"type": "message", "id": "00000003", "parentId": "00000002", "timestamp": now,
         "message": {"role": "user", "content": "Wait for my branch choice", "timestamp": 1}},
        {"type": "message", "id": "00000004", "parentId": "00000003", "timestamp": now,
         "message": {"role": "assistant", "content": [{"type": "text", "text": "Which branch should I use?"}],
                     "api": "recovery-smoke-api", "provider": "recovery-smoke", "model": "fixture", "usage": usage, "stopReason": "stop", "timestamp": 2}},
    ]
    session.write_text("".join(json.dumps(entry) + "\n" for entry in entries))
    session.chmod(0o600)
    profile = ["--offline", "-ne", "-ns", "-np", "-nc", "-na", "--no-tools", "-e", str(provider),
               "-e", str(dialogs), "--provider", "recovery-smoke", "--model", "fixture", "--thinking", "off"]
    children, masters = [], []

    def cli(*args):
        result = subprocess.run([str(CLI), "--state-dir", str(state_dir), *args], cwd=project,
                                env=env, capture_output=True, timeout=10)
        value = json.loads(result.stdout)
        assert result.returncode == 0, value
        return value["result"]

    cli("disable", "--session", str(session))
    original_policy = (state_dir / "state.json").read_bytes()
    original_history = session.read_bytes()

    def start():
        if events.exists():
            events.unlink()
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
        child = subprocess.Popen([str(CLI), "--state-dir", str(state_dir), "handoff-pi", "--session", str(session),
                                  "--pi-program", str(launcher), "--", *profile],
                                 cwd=project, env=env, stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
        os.close(slave)
        children.append(child)
        masters.append(master)
        return child, master

    def pump():
        for master in masters:
            try:
                if select.select([master], [], [], 0)[0]:
                    chunk = os.read(master, 65536)  # Never persist terminal output.
                    if b"\x1b[6n" in chunk:
                        os.write(master, b"\x1b[1;1R")
                    if b"\x1b]11;?" in chunk:
                        os.write(master, b"\x1b]11;rgb:0000/0000/0000\x1b\\")
            except OSError:
                pass

    def marks():
        return [json.loads(line) for line in events.read_text().splitlines()] if events.exists() else []

    def wait(predicate, description):
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            pump()
            if predicate():
                return
            time.sleep(.02)
        raise AssertionError("timeout: " + description + "; fixture events: " + str(marks()))

    def idle(seconds=.7):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            pump()
            time.sleep(.02)

    def kill(child):
        if child.poll() is None:
            child.kill()  # Precisely the known native PID after exec, never a group.
        child.wait(timeout=5)

    def answers(event):
        return [mark["value"] for mark in marks() if mark["event"] == event]

    try:
        # Abrupt native loss while a real selector is pending: no input/answer/model.
        first, terminal = start()
        wait(lambda: any(mark["event"] == "select_waiting" for mark in marks()), "real native selector")
        idle()
        assert first.poll() is None
        assert all(mark["pid"] == first.pid and mark["mode"] == "tui" for mark in marks())
        assert not any(mark["event"].endswith("_answer") for mark in marks())
        assert not calls.exists()
        kill(first)
        assert session.read_bytes() == original_history
        assert (state_dir / "state.json").read_bytes() == original_policy

        # Fresh exact-file native client receives only explicit fixture keyboard answers.
        second, terminal = start()
        wait(lambda: any(mark["event"] == "select_waiting" for mark in marks()), "reopened selector")
        idle()
        assert not any(mark["event"].endswith("_answer") for mark in marks())
        os.write(terminal, b"\x1b[B\r")  # Explicitly select experiment.
        wait(lambda: answers("select_answer"), "operator selection")
        assert answers("select_answer") == ["experiment"]
        wait(lambda: any(mark["event"] == "confirm_waiting" for mark in marks()), "native confirmation")
        idle()
        assert answers("confirm_answer") == []
        os.write(terminal, b"\x1b[B\r")  # Explicit No, not an implicit approval.
        wait(lambda: answers("confirm_answer"), "operator confirmation")
        assert answers("confirm_answer") == [False]
        wait(lambda: any(mark["event"] == "input_waiting" for mark in marks()), "native input")
        idle()
        assert answers("input_answer") == []
        os.write(terminal, b"operator fixture task\r")
        wait(lambda: answers("input_answer"), "operator text")
        assert answers("input_answer") == ["operator fixture task"]
        wait(lambda: any(mark["event"] == "editor_waiting" for mark in marks()), "native editor")
        idle()
        assert answers("editor_answer") == []
        os.write(terminal, b"\x1b")  # Explicitly cancel editor.
        wait(lambda: any(mark["event"] == "ready" for mark in marks()), "editor cancellation")
        assert answers("editor_answer") == [None]
        assert not calls.exists()
        assert session.read_bytes() == original_history
        assert (state_dir / "state.json").read_bytes() == original_policy
        os.write(terminal, b"busy fixture: explicit operator work\r")
        wait(lambda: calls.exists(), "explicit offline model work")
        idle(.3)
        assert [json.loads(line) for line in calls.read_text().splitlines()] == [{"recovery": False}]
        kill(second)  # Abrupt loss while fixture provider work is active.
        report = cli("assess-pi", "--session", str(session))
        assert report["eligible"] is False and report["actions"]["submit_continuation"] is False
        assert report["policy_allowed"] is False
        assert json.loads(session.read_text().splitlines()[0])["id"] == native_id
        interrupted_history = session.read_bytes()

        third, _ = start()
        wait(lambda: any(mark["event"] == "select_waiting" for mark in marks()), "repeated reopen selector")
        idle(1)
        assert third.poll() is None
        assert not any(mark["event"].endswith("_answer") for mark in marks())
        assert session.read_bytes() == interrupted_history
        assert [json.loads(line) for line in calls.read_text().splitlines()] == [{"recovery": False}]
        assert (state_dir / "state.json").read_bytes() == original_policy
        kill(third)
    finally:
        for child in children:
            kill(child)
        for master in masters:
            os.close(master)

print(f"PASS: Pi {metadata['version']} native TUI handoff, explicit dialogs, SIGKILL at wait/active work, repeated exact-file reopen without continuation, unchanged disable; no tmux/Reignite extension/network costs/reboot")
