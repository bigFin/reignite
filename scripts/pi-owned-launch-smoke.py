#!/usr/bin/env python3
"""Research only: real SDK/TUI, stop-before-forward, backend-held managed lock.
No automatic continuation, installation, product extension, network or reboot.
The lock is cooperative: this does NOT claim exclusive ownership against bare Pi.
"""
import datetime
import fcntl
import json
import os
import pathlib
import pty
import select
import shutil
import struct
import subprocess
import tempfile
import termios
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parent.parent
PACKAGE = pathlib.Path(os.environ["PI_PACKAGE_DIR"]).resolve()
VERSION = json.loads((PACKAGE / "package.json").read_text())["version"]
CLI = (pathlib.Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "debug/reignite").resolve()

with tempfile.TemporaryDirectory(prefix="reignite-pi-owned-") as temporary:
    base = pathlib.Path(temporary).resolve()
    home, project = base / "home", base / "project with spaces"
    home.mkdir()
    project.mkdir()
    agent_dir = home / ".pi/agent"
    agent_dir.mkdir(parents=True)
    (agent_dir / "settings.json").write_text(json.dumps({"enableInstallTelemetry": False,
        "quietStartup": True, "cacheWarming": "off", "retry": {"enabled": False},
        "compaction": {"enabled": False}}))
    provider, dialogs = base / "provider.ts", base / "dialogs.ts"
    shutil.copyfile(ROOT / "tests/pi-intent-provider.ts", provider)
    shutil.copyfile(ROOT / "tests/pi-handoff-dialog.ts", dialogs)
    host_events, provider_events = base / "host-events", base / "provider-events"
    dialog_events, state, lock_path = base / "dialog-events", base / "state", base / "managed-lock"
    session, native_id = base / "session.jsonl", str(uuid.uuid4())
    # Native-format fixture header only. Every conversation entry comes from Pi.
    session.write_text(json.dumps({"type": "session", "version": 3, "id": native_id,
        "cwd": str(project), "timestamp": datetime.datetime.now(datetime.timezone.utc).isoformat()}) + "\n")
    session.chmod(0o600)
    env = {"PATH": os.environ["PATH"], "HOME": str(home), "TERM": "xterm-256color", "PI_OFFLINE": "1",
        "PI_PACKAGE_DIR": str(PACKAGE), "PI_CODING_AGENT_DIR": str(agent_dir),
        "XDG_CONFIG_HOME": str(home / ".config"), "XDG_DATA_HOME": str(home / ".local/share"),
        "XDG_STATE_HOME": str(home / ".local/state"), "XDG_CACHE_HOME": str(home / ".cache"),
        "PI_OWNED_PROVIDER": str(provider), "PI_OWNED_REIGNITE": str(CLI),
        "PI_OWNED_SESSION": str(session), "PI_OWNED_STATE": str(state),
        "PI_OWNED_EVENTS": str(host_events), "PI_INTENT_EVENTS": str(provider_events),
        "PI_HANDOFF_EVENTS": str(dialog_events), "PI_INTENT_ABORT_DELAY_MS": "60000"}
    children, masters, bytes_read = [], [], 0

    def rows(path):
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def cli(*args):
        result = subprocess.run([str(CLI), "--state-dir", str(state), *args], cwd=project,
            env=env, capture_output=True, timeout=10)
        assert result.returncode == 0, "Reignite fixture command failed"
        return json.loads(result.stdout)["result"]

    def acquire():
        descriptor = os.open(lock_path, os.O_CREAT | os.O_RDWR | os.O_CLOEXEC, 0o600)
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            return descriptor
        except BaseException:
            os.close(descriptor)
            raise

    def start(show_dialogs=False):
        descriptor = acquire()
        try:
            master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
            try:
                child = subprocess.Popen(["node", str(ROOT / "tests/pi-owned-host.mjs")], cwd=project,
                    env={**env, "PI_OWNED_LOCK_FD": str(descriptor),
                         **({"PI_OWNED_DIALOGS": str(dialogs)} if show_dialogs else {})},
                    stdin=slave, stdout=slave, stderr=slave, pass_fds=(descriptor,), start_new_session=True)
            except BaseException:
                os.close(master)
                raise
            finally:
                os.close(slave)
            children.append(child)
            masters.append(master)
            return child, master
        finally:
            # The CLIENT no longer holds this lock. It lives in the native backend.
            os.close(descriptor)

    def pump():
        global bytes_read
        for master in masters:
            try:
                if select.select([master], [], [], 0)[0]:
                    chunk = os.read(master, 65536)
                    bytes_read += len(chunk)
                    assert bytes_read <= 4 * 1024 * 1024, "terminal fixture budget exceeded"
                    if b"\x1b[6n" in chunk:
                        os.write(master, b"\x1b[1;1R")
                    if b"\x1b]11;?" in chunk:
                        os.write(master, b"\x1b]11;rgb:0000/0000/0000\x1b\\")
            except OSError:
                pass  # Never display or persist terminal output.

    def wait(predicate, seconds=15):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            pump()
            if predicate():
                return
            assert all(child.poll() is None for child in children if child not in dead), "native host exited"
            time.sleep(.02)
        raise AssertionError("owned SDK/TUI fixture deadline exceeded")

    def idle(seconds=.7):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            pump()
            time.sleep(.02)

    dead = set()

    def kill(child):
        if child.poll() is None:
            fields = pathlib.Path(f"/proc/{child.pid}/stat").read_text().rsplit(")", 1)[1].split()
            assert int(fields[1]) == os.getpid(), "not our immediate native child"
            child.kill()  # Exactly this Popen child, never a group or name match.
        child.wait(timeout=5)
        dead.add(child)

    try:
        first, terminal = start()
        wait(lambda: any(row["event"] == "host_ready" for row in rows(host_events)))
        idle()
        assert not provider_events.exists()
        assert not (state / "state.json").exists()
        # A second managed launch cannot obtain the lock even after client FD close.
        try:
            descriptor = acquire()
        except BlockingIOError:
            pass
        else:
            os.close(descriptor)
            raise AssertionError("backend did not retain managed lock")
        os.write(terminal, b"Explicit offline work in the owned launcher\r")
        wait(lambda: any(row["event"] == "started" for row in rows(provider_events)))
        assert all(row["pid"] == first.pid for row in rows(provider_events))
        assert not (state / "state.json").exists(), "held before test Stop input"
        active_history = session.read_bytes()
        assert any(row.get("message", {}).get("role") == "user" for row in rows(session))
        # Normal native Escape: the controller saves the hold BEFORE forwarding it.
        os.write(terminal, b"\x1b")
        wait(lambda: any(row["event"] == "abort_seen" for row in rows(provider_events)))
        host_rows = rows(host_events)
        names = [row["event"] for row in host_rows]
        assert names.index("policy_saved_before_input") < names.index("native_abort_signal")
        assert all(row["pid"] == first.pid for row in host_rows)
        assert session.read_bytes() == active_history, "final abort unexpectedly persisted"
        report = cli("assess-pi", "--session", str(session))
        assert report["session"]["native_id"] == native_id
        assert report["policy_allowed"] is False and report["eligible"] is False
        assert report["decision"] == "observe_only"
        saved_policy = (state / "state.json").read_bytes()
        kill(first)  # Abrupt loss BEFORE Pi persists its final aborted message.
        assert session.read_bytes() == active_history
        assert (state / "state.json").read_bytes() == saved_policy

        # A fresh managed backend can now load, but it never starts model work.
        second, _ = start()
        wait(lambda: any(row["event"] == "host_ready" and row["pid"] == second.pid for row in rows(host_events)))
        idle()
        assert sum(row["event"] == "started" for row in rows(provider_events)) == 1
        assert session.read_bytes() == active_history
        assert (state / "state.json").read_bytes() == saved_policy
        kill(second)

        # Embedding the SDK retains Pi's actual extension dialog UI, unanswered.
        third, _ = start(show_dialogs=True)
        wait(lambda: any(row["event"] == "select_waiting" for row in rows(dialog_events)))
        idle()
        assert all(row["pid"] == third.pid and row["mode"] == "tui" for row in rows(dialog_events))
        assert not any(row["event"].endswith("_answer") for row in rows(dialog_events))
        assert sum(row["event"] == "started" for row in rows(provider_events)) == 1
        assert session.read_bytes() == active_history
        assert (state / "state.json").read_bytes() == saved_policy
        kill(third)
        descriptor = acquire()  # Kernel released only our dead backend's lock.
        os.close(descriptor)
    finally:
        for child in children:
            kill(child)
        for master in masters:
            os.close(master)

print(f"PASS: Pi {VERSION} public SDK/native TUI, durable policy hold before Escape/native abort, exact-PID loss before final persistence, no-work reopen, unanswered native selector, backend-held cooperative lock; not automatic recovery or exclusive fencing against bare Pi")
