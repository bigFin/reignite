#!/usr/bin/env python3
"""Real upstream restore transport, private tmux socket, stub Pi (no model calls).
This proves send-keys carries the ticket/path; pi-smoke.py owns native API proof.
"""
import json
import os
import pathlib
import shlex
import shutil
import subprocess
import tempfile
import time

plugin = pathlib.Path(os.environ["TMUX_ASSISTANT_PLUGIN_DIR"])
tmux = shutil.which("tmux")
assert tmux
with tempfile.TemporaryDirectory(prefix="recovery-tmux-") as temporary:
    base = pathlib.Path(temporary)
    socket = base / "private.socket"
    binary = base / "bin"
    binary.mkdir()
    home = base / "home"
    home.mkdir()
    project = base / "project with spaces"
    project.mkdir()
    output = base / "argv.json"
    sidecar = base / "assistant-sessions.json"
    session = project / "session with spaces.jsonl"
    session_id = "00000000-0000-4000-8000-000000000004"
    ticket = "00000000-0000-4000-8000-000000000003"
    shim = binary / "tmux"
    shim.write_text("#!/usr/bin/env bash\nexec " + shlex.quote(tmux) + " -S " + shlex.quote(str(socket)) + ' "$@"\n')
    shim.chmod(0o700)
    stub = binary / "pi"
    stub.write_text("#!/usr/bin/env python3\nimport json,os,sys\nwith open(os.environ['TRANSPORT_OUTPUT'],'w') as f: json.dump({'argv':sys.argv[1:],'cwd':os.getcwd()},f)\n")
    stub.chmod(0o700)
    env = {"HOME": str(home), "PATH": str(binary) + ":" + os.environ["PATH"], "TERM": "xterm-256color", "TMUX_RESURRECT_DIR": str(base), "TRANSPORT_OUTPUT": str(output)}
    sidecar.write_text(json.dumps({"sessions": [{"tool": "pi", "pane": "fixture:0.0", "session_id": session_id, "recovery_session_file": str(session), "cwd": str(project), "cli_args": "--recovery-interactive --recovery-ticket=" + ticket}]}))

    def call(*args):
        return subprocess.run([str(shim), *args], env=env, check=True, capture_output=True, text=True)

    try:
        call("-f", "/dev/null", "new-session", "-d", "-s", "fixture", "-c", str(project), "bash --noprofile --norc")
        subprocess.run(["bash", str(plugin / "scripts/restore-assistant-sessions.sh")], env=env, check=True, capture_output=True, text=True, timeout=30)
        deadline = time.monotonic() + 5
        while not output.exists() and time.monotonic() < deadline:
            time.sleep(.05)
        if not output.exists():
            log = base / "assistant-restore.log"
            details = log.read_text() if log.exists() else "no upstream restore log"
            raise AssertionError("Upstream restore did not launch Pi: " + details)
        received = json.loads(output.read_text())
        assert received == {"argv": ["--recovery-interactive", "--recovery-ticket=" + ticket, "--session", session_id], "cwd": str(project)}, received
        print("PASS: real upstream restore carried one ticket and native session ID through private tmux, with cwd spaces; stub Pi only")
    finally:
        subprocess.run([str(shim), "kill-server"], env=env, check=False, capture_output=True)
