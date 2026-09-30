#!/usr/bin/env python3
"""Real installed Pi TUI + local provider + real Rust CLI in disposable HOME/state.
Boot changes are simulated in the disposable durable record, never the kernel.
"""
import json
import fcntl
import termios
import struct
import os
import pathlib
import pty
import select
import shutil
import signal
import subprocess
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
PACKAGE = pathlib.Path(os.environ["PI_PACKAGE_DIR"])
metadata = json.loads((PACKAGE / "package.json").read_text())
entrypoint = PACKAGE / metadata["bin"]["pi"]
print(f'Pi fixture: {metadata["version"]} ({entrypoint})', flush=True)
CLI = pathlib.Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "debug/reignite"
CLI = CLI.resolve()

with tempfile.TemporaryDirectory(prefix="pi-recovery-smoke-") as temporary:
    base = pathlib.Path(temporary)
    home = base / "home"
    project = base / "project with spaces"
    home.mkdir()
    project.mkdir()
    state = base / "state"
    observations = base / "observations.jsonl"
    env = {"PATH": os.environ["PATH"], "HOME": str(home), "TERM": "xterm-256color", "PI_OFFLINE": "1", "PI_CODING_AGENT_DIR": str(home / ".pi/agent"), "REIGNITE_CLI": str(CLI), "REIGNITE_STATE_DIR": str(state), "RECOVERY_SMOKE_OBSERVATIONS": str(observations)}
    children = []
    masters = []
    terminal_tail = bytearray()
    provider = base / "fake-provider.ts"
    # Pi's loader resolves the installed pi-ai package itself, not npm downloads.
    shutil.copyfile(ROOT / "tests/fake-provider.ts", provider)
    args = ["node", str(entrypoint), "--offline", "-ne", "-ns", "-np", "-nc", "-na", "--no-tools", "-e", str(provider), "-e", str(ROOT / "adapters/pi.ts"), "--recovery-interactive", "--provider", "recovery-smoke", "--model", "fixture"]

    def start(extra=()):
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
        child = subprocess.Popen(args + list(extra), cwd=project, env=env, stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
        os.close(slave)
        children.append(child)
        masters.append(master)
        return child, master

    def pump():
        for master in masters:
            try:
                if select.select([master], [], [], 0)[0]:
                    chunk = os.read(master, 65536)  # Never persist terminal/session output.
                    terminal_tail.extend(chunk)
                    del terminal_tail[:-4000]
                    if b"\x1b[6n" in chunk:
                        os.write(master, b"\x1b[1;1R")
                    if b"\x1b]11;?" in chunk:
                        os.write(master, b"\x1b]11;rgb:0000/0000/0000\x1b\\")
            except OSError:
                pass

    def wait(predicate, description):
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            pump()
            try:
                if predicate():
                    return
            except (FileNotFoundError, IndexError, json.JSONDecodeError):
                pass
            time.sleep(.05)
        raise AssertionError("timeout: " + description + "; disposable terminal: " + repr(bytes(terminal_tail)))

    def record():
        return next(iter(json.loads((state / "state.json").read_text())["records"].values()))

    def seen():
        return [json.loads(line) for line in observations.read_text().splitlines()]

    def command(*argv):
        result = subprocess.run([str(CLI), "--state-dir", str(state), *argv], cwd=project, env=env, capture_output=True, text=True)
        response = json.loads(result.stdout)
        assert result.returncode == 0, response
        return response["result"]

    def kill(child):
        if child.poll() is None:
            os.killpg(child.pid, signal.SIGKILL)
        child.wait(timeout=5)

    try:
        original, terminal = start()
        for _ in range(40):
            pump()
            time.sleep(.05)
        os.write(terminal, b"persist fixture\r")
        wait(lambda: len(seen()) == 1, "first local provider call")
        time.sleep(.5)
        os.write(terminal, b"/recovery enable\r")
        wait(lambda: record()["enabled"], "explicit opt-in")
        os.write(terminal, b"busy fixture\r")
        wait(lambda: len(seen()) == 2 and record()["activity"] == "busy", "active interrupted run")
        session = record()["identity"]["session_file"]
        session_id = record()["identity"]["session_id"]
        kill(original)  # Abrupt process loss, NOT a real host reboot.
        database = json.loads((state / "state.json").read_text())
        for r in database["records"].values():
            r["owner"]["boot"] = "00000000-0000-4000-8000-000000000001"
        (state / "state.json").write_text(json.dumps(database))
        assert command("recover", "--session", session, "--dry-run")["eligible"]
        ticket = command("recover", "--session", session)["ticket"]
        restored, _ = start(["--session", session_id, "--recovery-ticket=" + ticket])
        wait(lambda: sum(o["recovery"] for o in seen()) == 1, "API recovery continuation")
        wait(lambda: record()["activity"] == "idle" and record()["attempt"]["status"] == "accepted", "final settlement")
        assert record()["identity"]["session_file"] == session
        duplicate, _ = start(["--session", session_id, "--recovery-ticket=" + ticket])
        for _ in range(40):
            pump()
            time.sleep(.05)
        assert sum(o["recovery"] for o in seen()) == 1
        kill(duplicate)
        kill(restored)
        manual, _ = start(["--session", session_id])
        wait(lambda: record()["owner"]["pid"] == manual.pid and not record()["enabled"], "manual reopen disarms")
        time.sleep(.5)
        pump()
        assert sum(o["recovery"] for o in seen()) == 1
        print("PASS: real Pi TUI persisted opt-in, abrupt loss, simulated boot, one API continuation, settled idle, duplicate/manual suppression; zero network/model costs")
    finally:
        for child in children:
            kill(child)
        for master in masters:
            os.close(master)
