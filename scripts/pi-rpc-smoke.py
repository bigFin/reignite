#!/usr/bin/env python3
"""Real Pi RPC in disposable HOME: exact-file reopen, no work, unanswered UI.
This is an API compatibility test, not a Reignite Pi delivery connector or reboot.
"""
import datetime
import hashlib
import json
import os
import pathlib
import selectors
import shutil
import subprocess
import tempfile
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parent.parent
PACKAGE = pathlib.Path(os.environ["PI_PACKAGE_DIR"]).resolve()
metadata = json.loads((PACKAGE / "package.json").read_text())
entrypoint = PACKAGE / metadata["bin"]["pi"]
CLI = (pathlib.Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "debug/reignite").resolve()

with tempfile.TemporaryDirectory(prefix="reignite-pi-rpc-") as temporary:
    base = pathlib.Path(temporary).resolve()
    home, project = base / "home", base / "project with spaces"
    home.mkdir()
    project.mkdir()
    provider = base / "provider.ts"
    dialogs = base / "dialogs.ts"
    shutil.copyfile(ROOT / "tests/fake-provider.ts", provider)
    shutil.copyfile(ROOT / "tests/pi-rpc-dialog.ts", dialogs)
    observations, answers = base / "model-calls", base / "dialog-answers"
    env = {
        "PATH": os.environ["PATH"], "HOME": str(home), "PI_OFFLINE": "1",
        "PI_CODING_AGENT_DIR": str(home / ".pi/agent"),
        "XDG_CONFIG_HOME": str(home / ".config"), "XDG_DATA_HOME": str(home / ".local/share"),
        "XDG_STATE_HOME": str(home / ".local/state"), "XDG_CACHE_HOME": str(home / ".cache"),
        "RECOVERY_SMOKE_OBSERVATIONS": str(observations), "PI_RPC_DIALOG_ANSWERS": str(answers),
    }
    session = base / "selected-session.jsonl"
    native_id = str(uuid.uuid4())
    now = datetime.datetime.now(datetime.timezone.utc).isoformat()
    usage = {key: 0 for key in ("input", "output", "cacheRead", "cacheWrite", "totalTokens")}
    usage["cost"] = {key: 0 for key in ("input", "output", "cacheRead", "cacheWrite", "total")}
    entries = [
        {"type": "session", "version": 3, "id": native_id, "cwd": str(project), "timestamp": now},
        {"type": "model_change", "id": "00000001", "parentId": None, "timestamp": now,
         "provider": "recovery-smoke", "modelId": "fixture"},
        {"type": "thinking_level_change", "id": "00000002", "parentId": "00000001",
         "timestamp": now, "thinkingLevel": "off"},
        {"type": "message", "id": "00000003", "parentId": "00000002", "timestamp": now,
         "message": {"role": "user", "content": [{"type": "text", "text": "Wait for my branch choice."}], "timestamp": 1}},
        {"type": "message", "id": "00000004", "parentId": "00000003", "timestamp": now,
         "message": {"role": "assistant", "content": [{"type": "text", "text": "Which branch should I use?"}],
                     "api": "recovery-smoke-api", "provider": "recovery-smoke", "model": "fixture",
                     "usage": usage, "stopReason": "stop", "timestamp": 2}},
    ]
    session.write_text("".join(json.dumps(entry) + "\n" for entry in entries))
    session.chmod(0o600)
    before = hashlib.sha256(session.read_bytes()).digest()
    args = ["node", str(entrypoint), "--mode", "rpc", "--offline", "-ne", "-ns", "-np", "-nc", "-na",
            "--no-tools", "-e", str(provider), "--provider", "recovery-smoke", "--model", "fixture",
            "--thinking", "off", "--session", str(session)]

    def inspect():
        result = subprocess.run([str(CLI), "--state-dir", str(base / "recovery-state"),
                                 "inspect", "--session", str(session)], env=env, capture_output=True, check=True)
        report = json.loads(result.stdout)["result"]
        assert report["session"]["native_id"] == native_id
        assert report["session"]["last_persisted_entry"] == "00000004"
        assert report["eligible"] is False
        assert not (base / "recovery-state").exists()

    def assess():
        result = subprocess.run([str(CLI), "--state-dir", str(base / "assessment-state"),
                                 "assess-pi", "--session", str(session)], env=env, capture_output=True, check=True)
        report = json.loads(result.stdout)["result"]
        assert report["eligible"] is False and report["decision"] == "observe_only"
        assert report["actions"]["submit_continuation"] is False
        assert any(item["fact"] == "human_decision_state" and item["state"] == "missing"
                   for item in report["requirements"])
        assert not (base / "assessment-state/state.json").exists()
        return report["requirements"]

    inspect()
    baseline_requirements = assess()
    for with_dialogs in (False, True, True):
        child = subprocess.Popen(args + (["-e", str(dialogs)] if with_dialogs else []),
                                 cwd=project, env=env, stdin=subprocess.PIPE,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        def owned_start():
            fields = pathlib.Path(f"/proc/{child.pid}/stat").read_text().rsplit(")", 1)[1].split()
            assert int(fields[1]) == os.getpid(), "not our immediate test child"
            return fields[19]
        start_identity = owned_start()
        selector = selectors.DefaultSelector()
        selector.register(child.stdout, selectors.EVENT_READ, "stdout")
        selector.register(child.stderr, selectors.EVENT_READ, "stderr")
        pending, records, diagnostics = bytearray(), [], bytearray()
        bytes_read = 0
        sent = []

        def receive_until(predicate, seconds=15):
            global bytes_read
            deadline = time.monotonic() + seconds
            while not predicate():
                assert time.monotonic() < deadline, f"RPC timeout: {bytes(diagnostics)!r}"
                if child.poll() is not None:
                    assert predicate(), f"RPC exited: {bytes(diagnostics)!r}"
                    return
                for key, _ in selector.select(min(.1, max(0, deadline - time.monotonic()))):
                    chunk = os.read(key.fileobj.fileno(), 65536)
                    if not chunk:
                        selector.unregister(key.fileobj)
                        continue
                    if key.data == "stderr":
                        diagnostics.extend(chunk)
                        del diagnostics[:-8192]
                        continue
                    bytes_read += len(chunk)
                    assert bytes_read <= 1024 * 1024, "RPC wire budget exceeded"
                    pending.extend(chunk)
                    assert len(pending) <= 128 * 1024, "RPC record budget exceeded"
                    while b"\n" in pending:
                        line, _, rest = pending.partition(b"\n")
                        pending[:] = rest
                        record = json.loads(line.rstrip(b"\r"))
                        assert record.get("type") not in {"agent_start", "turn_start", "tool_execution_start"}
                        records.append(record)

        def request(kind):
            assert kind in {"get_state", "get_entries", "get_messages"}
            ident = "inspect-" + kind
            command = {"id": ident, "type": kind}
            sent.append(command)
            child.stdin.write(json.dumps(command).encode() + b"\n")
            child.stdin.flush()
            receive_until(lambda: any(record.get("id") == ident for record in records))
            response = next(record for record in records if record.get("id") == ident)
            assert response["type"] == "response" and response["command"] == kind
            assert response["success"] is True
            return response["data"]

        try:
            state = request("get_state")
            assert state["sessionId"] == native_id and state["sessionFile"] == str(session)
            assert state["isStreaming"] is False and state["pendingMessageCount"] == 0
            assert state["isCompacting"] is False
            history = request("get_entries")
            assert history["leafId"] == "00000004"
            assert [entry["id"] for entry in history["entries"]] == [entry["id"] for entry in entries[1:]]
            messages = request("get_messages")["messages"]
            assert messages[-1]["content"][0]["text"] == "Which branch should I use?"
            if with_dialogs:
                receive_until(lambda: {record.get("method") for record in records
                                      if record.get("type") == "extension_ui_request"}
                              >= {"select", "confirm", "input", "editor"})
                # Requests are visible to a pipe-owning client, but we send no UI responses.
                assert not answers.exists(), "a human dialog resolved without an answer"
            # The same history and idle/empty-queue projection exist with and without
            # outstanding native dialogs. Neither can prove crash-time human-wait state.
            assert assess() == baseline_requirements
            assert [command["type"] for command in sent] == ["get_state", "get_entries", "get_messages"]
            assert not observations.exists(), "model work started during inspection/reopen"
            assert hashlib.sha256(session.read_bytes()).digest() == before
            child.stdin.close()
            # Keep draining stdout/stderr during shutdown; do not deadlock on backpressure.
            while child.poll() is None:
                receive_until(lambda: child.poll() is not None, seconds=5)
            assert child.wait(timeout=5) == 0
        finally:
            selector.close()
            if child.poll() is None:
                assert owned_start() == start_identity, "test PID/start identity changed"
                child.kill()  # Exactly the tracked immediate child; never a process group.
            child.wait(timeout=5)
            for pipe in (child.stdin, child.stdout, child.stderr):
                if pipe and not pipe.closed:
                    pipe.close()
        # Disposal may cancel unresolved UI promises; it must never select/approve.
        if answers.exists():
            for line in answers.read_text().splitlines():
                answer = json.loads(line)
                if answer["method"] == "confirm":
                    assert answer["value"] is False or answer["value"] is None
                else:
                    assert answer["value"] is None
            answers.unlink()
        assert not observations.exists()
        assert hashlib.sha256(session.read_bytes()).digest() == before
        inspect()
    print(f'PASS: Pi {metadata["version"]} RPC, exact-file reopen x3, four unanswered dialog kinds, zero model calls; no tmux/reboot.')
