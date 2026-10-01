#!/usr/bin/env python3
"""Real Pi: pending cancellation is not durable intent or exclusive ownership.
Only disposable exact-PID children; no legacy enrollment, network, costs or reboot.
The provider's markers are test ground truth, never inputs to Reignite.
"""
import datetime
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
ENTRY = PACKAGE / metadata["bin"]["pi"]
CLI = (pathlib.Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "debug/reignite").resolve()


class OwnedPi:
    """A bounded test peer, not a product control client or recovery authority."""
    def __init__(self, args, cwd, env):
        self.child = subprocess.Popen(args, cwd=cwd, env=env, stdin=subprocess.PIPE,
                                      stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.child.stdout, selectors.EVENT_READ, "stdout")
        self.selector.register(self.child.stderr, selectors.EVENT_READ, "stderr")
        self.records, self.sent = [], {}
        self.pending = bytearray()
        self.bytes_read = 0
        self.start = self.signature()
        self.deadline = time.monotonic() + 60

    def signature(self):
        fields = pathlib.Path(f"/proc/{self.child.pid}/stat").read_text().rsplit(")", 1)[1].split()
        assert int(fields[1]) == os.getpid(), "not our immediate child"
        return fields[19]

    def receive(self, predicate, seconds=15):
        deadline = min(self.deadline, time.monotonic() + seconds)
        while not predicate():
            assert time.monotonic() < deadline, "native RPC fixture deadline exceeded"
            assert self.child.poll() is None, "native RPC fixture exited"
            for key, _ in self.selector.select(.02):
                chunk = os.read(key.fileobj.fileno(), 65536)
                if not chunk:
                    self.selector.unregister(key.fileobj)
                    continue
                self.bytes_read += len(chunk)
                assert self.bytes_read <= 4 * 1024 * 1024, "RPC fixture wire budget exceeded"
                if key.data == "stderr":
                    continue  # Drain without displaying or persisting diagnostics.
                self.pending.extend(chunk)
                assert len(self.pending) <= 1024 * 1024, "RPC fixture record budget exceeded"
                while b"\n" in self.pending:
                    line, _, remaining = self.pending.partition(b"\n")
                    self.pending[:] = remaining
                    record = json.loads(line.rstrip(b"\r"))
                    assert isinstance(record, dict)
                    assert record.get("type") != "extension_error", "fixture extension failed"
                    if record.get("type") == "response":
                        assert self.sent.get(record.get("id")) == record.get("command"), "foreign RPC response"
                    self.records.append(record)
                    assert len(self.records) <= 8192, "RPC fixture event budget exceeded"

    def send(self, kind, **params):
        assert kind in {"get_state", "get_entries", "get_messages", "prompt", "abort"}
        ident = f"fixture-{len(self.sent)}"
        self.sent[ident] = kind
        self.child.stdin.write(json.dumps({"id": ident, "type": kind, **params}).encode() + b"\n")
        self.child.stdin.flush()
        return ident

    def has_response(self, ident):
        return any(row.get("type") == "response" and row.get("id") == ident for row in self.records)

    def response(self, ident):
        self.receive(lambda: self.has_response(ident))
        row = next(row for row in self.records if row.get("type") == "response" and row.get("id") == ident)
        assert row["success"] is True, "native RPC command failed"
        return row.get("data")

    def query(self, kind, **params):
        return self.response(self.send(kind, **params))

    def kill(self):
        if self.child.poll() is None:
            assert self.signature() == self.start, "test PID/start identity changed"
            self.child.kill()  # Exactly our Popen child, never pkill or a group signal.
        self.child.wait(timeout=5)

    def close(self):
        try:
            self.kill()
        finally:
            self.selector.close()
            for pipe in (self.child.stdin, self.child.stdout, self.child.stderr):
                pipe.close()


with tempfile.TemporaryDirectory(prefix="reignite-pi-intent-") as temporary:
    base = pathlib.Path(temporary).resolve()
    home, project = base / "home", base / "project with spaces"
    home.mkdir()
    project.mkdir()
    provider = base / "provider.ts"
    shutil.copyfile(ROOT / "tests/pi-intent-provider.ts", provider)
    events = base / "events"
    usage_log = base / "usage.jsonl"
    usage_log.touch(mode=0o600)
    session = base / "session.jsonl"
    native_id = str(uuid.uuid4())
    now = datetime.datetime.now(datetime.timezone.utc).isoformat()
    rows = [
        {"type": "session", "version": 3, "id": native_id, "cwd": str(project), "timestamp": now},
        {"type": "model_change", "id": "model1", "parentId": None, "timestamp": now,
         "provider": "intent-fixture", "modelId": "fixture"},
        {"type": "thinking_level_change", "id": "thinking1", "parentId": "model1", "timestamp": now,
         "thinkingLevel": "off"},
    ]
    session.write_text("".join(json.dumps(row) + "\n" for row in rows))
    session.chmod(0o600)
    env = {"PATH": os.environ["PATH"], "HOME": str(home), "PI_OFFLINE": "1",
           "PI_CODING_AGENT_DIR": str(home / ".pi/agent"),
           "XDG_CONFIG_HOME": str(home / ".config"), "XDG_DATA_HOME": str(home / ".local/share"),
           "XDG_STATE_HOME": str(home / ".local/state"), "XDG_CACHE_HOME": str(home / ".cache"),
           "PI_INTENT_EVENTS": str(events), "PI_INTENT_ABORT_DELAY_MS": "60000",
           "PI_INTENT_USAGE_LOG": str(usage_log)}
    args = ["node", str(ENTRY), "--mode", "rpc", "--offline", "-ne", "-ns", "-np", "-nc", "-na",
            "--no-tools", "-e", str(provider), "--provider", "intent-fixture", "--model", "fixture",
            "--thinking", "off", "--session", str(session)]
    owned = []

    def start(extra_env=None, source=session):
        child = OwnedPi([*args[:-1], str(source)], project, {**env, **(extra_env or {})})
        owned.append(child)
        return child

    def marks():
        return [json.loads(line) for line in events.read_text().splitlines()] if events.exists() else []

    def assess(source=session):
        before = source.read_bytes()
        before_usage = usage_log.read_bytes()
        result = subprocess.run([str(CLI), "--state-dir", str(base / "assessment-state"),
                                 "assess-pi", "--session", str(source),
                                 "--usage-log", str(usage_log)], env=env,
                                capture_output=True, timeout=10)
        assert result.returncode == 0, "native assessment failed"
        report = json.loads(result.stdout)["result"]
        assert report["session"]["native_id"] == native_id
        assert report["eligible"] is False and report["decision"] == "observe_only"
        assert report["recovery_gate"]["automatic_candidate"] is False
        assert report["recovery_gate"]["usage_log"]["selected"] is True
        assert report["recovery_gate"]["usage_log"]["coverage"].startswith("partial_")
        for action, allowed in report["actions"].items():
            assert allowed == (action == "inspect")
        for fact in ("interrupted_work_intent_and_cancellation", "human_decision_state", "exclusive_execution_owner"):
            assert any(row["fact"] == fact and row["state"] == "missing" for row in report["requirements"])
        assert not (base / "assessment-state/state.json").exists()
        assert source.read_bytes() == before
        assert usage_log.read_bytes() == before_usage
        return report

    try:
        original = start()
        assert original.query("get_state")["isStreaming"] is False
        assert original.query("prompt", message="Keep this offline fixture active") == {"disposition": "started"}
        original.receive(lambda: any(mark["event"] == "started" for mark in marks()))
        before_state = original.query("get_state")
        assert before_state["isStreaming"] is True
        assert before_state["pendingMessageCount"] == 0
        before_entries = original.query("get_entries")
        before_messages = original.query("get_messages")
        before_history = session.read_bytes()
        before_assessment = assess()
        assert before_assessment["persisted_lineage"]["last_message_role"] == "user"
        assert before_assessment["recovery_gate"]["decision"] == "hold"
        assert usage_log.read_bytes() == b""

        # The native abort command actually reaches the provider's abort signal.
        # Hold the provider's final result long enough to inspect the unwind gap.
        abort_id = original.send("abort")
        original.receive(lambda: any(mark["event"] == "abort_seen" for mark in marks()))
        assert all(mark["pid"] == original.child.pid for mark in marks())
        assert original.query("get_state") == before_state
        assert original.query("get_entries") == before_entries
        assert original.query("get_messages") == before_messages
        assert not original.has_response(abort_id), "abort already settled"
        assert not any(row.get("type") == "agent_settled" for row in original.records)
        assert session.read_bytes() == before_history
        pending = assess()
        assert pending["requirements"] == before_assessment["requirements"]
        assert pending["recovery_gate"]["decision"] == "hold"
        assert usage_log.read_bytes() == b"", "usage logged cancellation before native message_end"

        # Native RPC load does not fence a second owner. Send no work to it.
        second = start()
        second_state = second.query("get_state")
        assert second_state["sessionId"] == native_id and second_state["sessionFile"] == str(session)
        assert second_state["isStreaming"] is False
        assert second.query("get_entries")["leafId"] == before_entries["leafId"]
        assert original.child.poll() is None and second.child.poll() is None
        assert sum(mark["event"] == "started" for mark in marks()) == 1
        assert session.read_bytes() == before_history
        assert assess()["requirements"] == before_assessment["requirements"]
        second.kill()

        # SIGKILL while cancellation is already requested loses that intent.
        original.kill()
        assert session.read_bytes() == before_history
        assert assess()["requirements"] == before_assessment["requirements"]
        reopened = start()
        assert reopened.query("get_state")["isStreaming"] is False
        assert reopened.query("get_entries") == before_entries
        assert reopened.query("get_messages") == before_messages
        assert sum(mark["event"] == "started" for mark in marks()) == 1
        assert session.read_bytes() == before_history
        reopened.kill()

        # Control: with normal provider unwinding, Pi DOES persist the abort.
        control = start({"PI_INTENT_ABORT_DELAY_MS": "30"})
        assert control.query("prompt", message="A separate explicit offline control task") == {"disposition": "started"}
        control.receive(lambda: sum(mark["event"] == "started" for mark in marks()) == 2)
        control.query("abort")
        control.receive(lambda: any(row.get("type") == "agent_settled" for row in control.records))
        assert control.query("get_state")["isStreaming"] is False
        report = assess()
        assert report["persisted_lineage"]["last_assistant_stop_reason"] == "aborted"
        assert any(row["code"] == "reported_cancel_or_error" for row in report["reasons"])
        assert report["recovery_gate"]["decision"] == "blocked"
        assert report["recovery_gate"]["usage_log"]["main_cancellations"] == 1
        assert any(row["code"] == "reported_work_cancelled" for row in report["recovery_gate"]["signals"])
        assert sum(mark["event"] == "started" for mark in marks()) == 2
        control.kill()

        # A separate fixture proves EXECUTION is not fenced at prompt time either.
        # Both prompts are explicit test inputs; neither is an automatic recovery.
        concurrent_source = base / "concurrent-session.jsonl"
        concurrent_source.write_text("".join(json.dumps(row) + "\n" for row in rows))
        concurrent_source.chmod(0o600)
        first_owner = start(source=concurrent_source)
        second_owner = start(source=concurrent_source)
        for owner in (first_owner, second_owner):
            assert owner.query("get_state")["isStreaming"] is False
        for owner in (first_owner, second_owner):
            assert owner.query("prompt", message="Explicit concurrent-owner fixture work") == {"disposition": "started"}
        second_owner.receive(lambda: sum(mark["event"] == "started" for mark in marks()) == 4)
        assert first_owner.child.pid != second_owner.child.pid
        for owner in (first_owner, second_owner):
            state = owner.query("get_state")
            assert state["sessionId"] == native_id and state["sessionFile"] == str(concurrent_source)
            assert state["isStreaming"] is True
            assert any(mark["event"] == "started" and mark["pid"] == owner.child.pid for mark in marks())
        assert first_owner.query("get_entries")["leafId"] != second_owner.query("get_entries")["leafId"]
        assess(concurrent_source)
    finally:
        for child in owned:
            child.close()

print(f"PASS: Pi {metadata['version']} pending abort has no history/usage evidence and stays on hold; normal persisted abort blocks the gate; exact-PID loss, two active RPC owners without fencing and no-work reopen; observation-only, no legacy enrollment/network/costs/reboot")
