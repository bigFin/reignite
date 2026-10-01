#!/usr/bin/env python3
"""Real Pi owned-RPC transport, offline provider, simulated boot/legacy ticket.
Not proof of native automatic eligibility, exclusive ownership or a real reboot.
No Reignite extension, tmux, user histories, network or model costs.
"""
import datetime
import json
import os
import pathlib
import selectors
import shlex
import shutil
import signal
import subprocess
import tempfile
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parent.parent
PACKAGE = pathlib.Path(os.environ["PI_PACKAGE_DIR"]).resolve()
metadata = json.loads((PACKAGE / "package.json").read_text())
ENTRY = PACKAGE / metadata["bin"]["pi"]
CLI = (pathlib.Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "debug/reignite").resolve()

with tempfile.TemporaryDirectory(prefix="reignite-pi-delivery-") as temporary:
    base = pathlib.Path(temporary).resolve()
    home, project = base / "home", base / "project with spaces"
    home.mkdir()
    project.mkdir()
    provider, dialogs = base / "provider.ts", base / "dialogs.ts"
    shutil.copyfile(ROOT / "tests/fake-provider.ts", provider)
    shutil.copyfile(ROOT / "tests/pi-rpc-dialog.ts", dialogs)
    launcher = base / "owned-pi"
    launcher.write_text("#!/usr/bin/env bash\nexec node " + shlex.quote(str(ENTRY)) + ' "$@"\n')
    launcher.chmod(0o700)
    observations, answers = base / "calls", base / "answers"
    env = {"PATH": os.environ["PATH"], "HOME": str(home), "PI_OFFLINE": "1",
           "PI_CODING_AGENT_DIR": str(home / ".pi/agent"),
           "XDG_CONFIG_HOME": str(home / ".config"), "XDG_DATA_HOME": str(home / ".local/share"),
           "XDG_STATE_HOME": str(home / ".local/state"), "XDG_CACHE_HOME": str(home / ".cache"),
           "RECOVERY_SMOKE_OBSERVATIONS": str(observations), "PI_RPC_DIALOG_ANSWERS": str(answers)}
    profile = ["--offline", "-ne", "-ns", "-np", "-nc", "-na", "--no-tools", "-e", str(provider),
               "--provider", "recovery-smoke", "--model", "fixture", "--thinking", "off"]
    session, native_id = base / "session.jsonl", str(uuid.uuid4())
    now = datetime.datetime.now(datetime.timezone.utc).isoformat()
    entries = [
        {"type": "session", "version": 3, "id": native_id, "cwd": str(project), "timestamp": now},
        {"type": "model_change", "id": "00000001", "parentId": None, "timestamp": now,
         "provider": "recovery-smoke", "modelId": "fixture"},
        {"type": "thinking_level_change", "id": "00000002", "parentId": "00000001",
         "timestamp": now, "thinkingLevel": "off"},
        {"type": "message", "id": "00000003", "parentId": "00000002", "timestamp": now,
         "message": {"role": "user", "content": [{"type": "text", "text": "Disposable transport fixture."}], "timestamp": 1}},
    ]
    session.write_text("".join(json.dumps(entry) + "\n" for entry in entries))
    session.chmod(0o600)
    state_dir = base / "state"

    def cli(*args, request=None, success=True):
        result = subprocess.run([str(CLI), "--state-dir", str(state_dir), *args], env=env,
                                input=json.dumps(request).encode() if request else None,
                                capture_output=True, timeout=20)
        value = json.loads(result.stdout)
        assert (result.returncode == 0) == success, value
        return value.get("result", value)

    def owner(op, identity, pid, **extra):
        return cli("request", request={"op": op, "identity": identity, "pid": pid, **extra})

    # Actual active native subprocess and leaf; only the reboot boundary is simulated.
    original = subprocess.Popen([str(launcher), *profile, "--mode", "rpc", "--session", str(session)],
                                cwd=project, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    selector = selectors.DefaultSelector()
    selector.register(original.stdout, selectors.EVENT_READ, "stdout")
    selector.register(original.stderr, selectors.EVENT_READ, "stderr")
    records, pending = [], bytearray()
    bytes_read = 0

    def receive(predicate):
        global bytes_read
        deadline = time.monotonic() + 15
        while not predicate():
            assert time.monotonic() < deadline and original.poll() is None, "native RPC fixture failed"
            for key, _ in selector.select(.1):
                chunk = os.read(key.fileobj.fileno(), 65536)
                if not chunk:
                    selector.unregister(key.fileobj)
                    continue
                bytes_read += len(chunk)
                assert bytes_read < 1024 * 1024
                if key.data == "stderr":
                    continue
                pending.extend(chunk)
                assert len(pending) <= 128 * 1024
                while b"\n" in pending:
                    line, _, remaining = pending.partition(b"\n")
                    pending[:] = remaining
                    records.append(json.loads(line.rstrip(b"\r")))

    def rpc(kind, **params):
        command = {"id": kind, "type": kind, **params}
        original.stdin.write(json.dumps(command).encode() + b"\n")
        original.stdin.flush()
        receive(lambda: any(row.get("id") == kind for row in records))
        response = next(row for row in records if row.get("id") == kind)
        assert response["type"] == "response" and response["command"] == kind and response["success"] is True
        return response.get("data")

    try:
        state = rpc("get_state")
        assert state["sessionId"] == native_id and state["sessionFile"] == str(session)
        identity = {"harness": "pi", "session_id": native_id, "session_file": str(session),
                    "cwd": str(project), "leaf": "00000003"}
        owner("open", identity, original.pid, ticket=None)
        owner("enable", identity, original.pid)
        rpc("prompt", message="busy fixture: keep this disposable work active")
        receive(lambda: any(row.get("type") == "agent_start" for row in records) and observations.exists())
        identity["leaf"] = rpc("get_entries")["leafId"]
        owner("observe", identity, original.pid, activity="busy")
    finally:
        # Abrupt loss of precisely our native subprocess, no global/group signals.
        original.kill()
        original.wait(timeout=5)
        selector.close()
        for pipe in (original.stdin, original.stdout, original.stderr):
            pipe.close()
    saved_source = session.read_bytes()
    database_path = state_dir / "state.json"
    database = json.loads(database_path.read_text())
    for record in database["records"].values():
        record["owner"]["boot"] = "00000000-0000-4000-8000-000000000001"
    database_path.write_text(json.dumps(database))
    ticket = cli("recover", "--session", str(session))["ticket"]
    checkpoint = database_path.read_bytes()

    def deliver(extra=(), success=True):
        return cli("deliver-pi", "--session", str(session), "--ticket", ticket,
                   "--pi-program", str(launcher), "--timeout-ms", "15000", "--", *profile, *extra, success=success)

    result = deliver()
    assert result["outcome"] == "settled" and result["api_acceptance_observed"] is True, result
    assert result["agent_settled_observed"] is True
    assert result["native_eligibility_verified"] is False and result["work_complete_verified"] is False
    calls = [json.loads(line) for line in observations.read_text().splitlines()]
    assert calls == [{"recovery": False}, {"recovery": True}], calls
    assert cli("status")["records"][0]["attempt"]["status"] == "accepted"
    deliver(success=False)
    assert observations.read_text().count('"recovery":true') + observations.read_text().count('"recovery": true') == 1

    # Real hard loss AFTER API acceptance: kill either our controller or the
    # exact owned native child. Only fixture boot metadata is simulated.
    for target in ("controller", "native"):
        session.write_bytes(saved_source)
        database_path.write_bytes(checkpoint)
        crash_env = {**env, "RECOVERY_SMOKE_DELAY_MS": "60000"}
        process = subprocess.Popen([str(CLI), "--state-dir", str(state_dir), "deliver-pi",
                                    "--session", str(session), "--ticket", ticket, "--pi-program", str(launcher),
                                    "--timeout-ms", "15000", "--", *profile], env=crash_env,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        native_pid, native_start = None, None
        previous_calls = observations.read_bytes()
        try:
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                record = json.loads(database_path.read_text())["records"]
                record = next(iter(record.values()))
                if record["attempt"]["status"] == "accepted" and observations.read_bytes() != previous_calls:
                    native_pid = record["owner"]["pid"]
                    native_start = record["owner"]["start"]
                    stat = pathlib.Path(f"/proc/{native_pid}/stat").read_text().rsplit(")", 1)[1].split()
                    assert int(stat[1]) == process.pid and stat[19] == native_start
                    break
                assert process.poll() is None, "delivery exited before acceptance"
                time.sleep(.005)
            assert native_pid is not None, "no accepted native delivery"
            if target == "controller":
                process.kill()
            else:
                os.kill(native_pid, signal.SIGKILL)
            process.communicate(timeout=5)
            # Controller death requests SIGTERM through PR_SET_PDEATHSIG. An
            # orphan zombie is terminated, not a surviving execution owner.
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                try:
                    stat = pathlib.Path(f"/proc/{native_pid}/stat").read_text().rsplit(")", 1)[1].split()
                    if stat[0] == "Z" or stat[19] != native_start:
                        break
                except FileNotFoundError:
                    break
                time.sleep(.01)
            else:
                raise AssertionError("owned native child survived controller loss")
            database = json.loads(database_path.read_text())
            for record in database["records"].values():
                assert record["attempt"]["status"] == "accepted"
                record["owner"]["boot"] = "00000000-0000-4000-8000-000000000001"
            database_path.write_text(json.dumps(database))
            assert cli("recover", "--session", str(session), "--dry-run")["eligible"] is False
            deliver(success=False)
            calls = [json.loads(line) for line in observations.read_text().splitlines()]
            assert sum(call["recovery"] for call in calls) == (2 if target == "controller" else 3)
        finally:
            if process.poll() is None:
                process.kill()
            process.communicate(timeout=5)
            if native_pid is not None:
                try:
                    stat = pathlib.Path(f"/proc/{native_pid}/stat").read_text().rsplit(")", 1)[1].split()
                    if stat[0] != "Z" and stat[19] == native_start:
                        os.kill(native_pid, signal.SIGKILL)
                except FileNotFoundError:
                    pass

    # Rewind PRIVATE test snapshots, never a product rearm operation.
    session.write_bytes(saved_source)
    database_path.write_bytes(checkpoint)
    result = deliver(["-e", str(dialogs)])
    assert result["outcome"] == "held" and result["api_acceptance_observed"] is False, result
    assert result["dialog_methods"], result
    assert session.read_bytes() == saved_source
    assert cli("status")["records"][0]["attempt"]["status"] == "authorized"
    assert [json.loads(line) for line in observations.read_text().splitlines()] == calls
    if answers.exists():
        for answer in map(json.loads, answers.read_text().splitlines()):
            assert answer["value"] is None or (answer["method"] == "confirm" and answer["value"] is False), answer

    # Fabricated busy projection must not override a persisted assistant question.
    database_path.write_bytes(checkpoint)
    usage = {key: 0 for key in ("input", "output", "cacheRead", "cacheWrite", "totalTokens")}
    usage["cost"] = {key: 0 for key in ("input", "output", "cacheRead", "cacheWrite", "total")}
    question = {"type": "message", "id": "question1", "parentId": identity["leaf"], "timestamp": now,
                "message": {"role": "assistant", "content": [{"type": "text", "text": "Choose a branch before proceeding"}],
                            "api": "recovery-smoke-api", "provider": "recovery-smoke", "model": "fixture",
                            "usage": usage, "stopReason": "stop", "timestamp": 2}}
    with session.open("a") as file:
        file.write(json.dumps(question) + "\n")
    database = json.loads(checkpoint)
    for record in database["records"].values():
        record["identity"]["leaf"] = "question1"
    database_path.write_text(json.dumps(database))
    before = session.read_bytes()
    result = deliver()
    assert result["outcome"] == "held" and result["reason"] == "waiting_or_uncertain_history", result
    assert session.read_bytes() == before
    assert [json.loads(line) for line in observations.read_text().splitlines()] == calls

print(f"PASS: Pi {metadata['version']} owned RPC delivery, one-use continuation/receipts, controller/native SIGKILL without retry, duplicate suppression, dialog/question holds; legacy-ticket transport/simulated boot only, no Reignite extension or tmux, zero network/model costs")
