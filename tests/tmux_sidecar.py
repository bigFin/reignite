"""Companion hook boundary tests with real CLI/files; reboot evidence is simulated."""
import json
import os
import pathlib
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent
CLI = (pathlib.Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "debug/reignite").resolve()
HOOK = ROOT / "integration/tmux-sidecar.py"


class SidecarContract(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="recovery-sidecar-")
        self.base = pathlib.Path(self.temporary.name)
        self.session = self.base / "session with spaces.jsonl"
        self.identity = {"harness": "pi", "session_id": "exact", "session_file": str(self.session), "cwd": str(pathlib.Path.cwd().resolve()), "leaf": "leaf1"}
        self.session.write_text(json.dumps({"type": "session", "version": 3, "id": "exact", "cwd": self.identity["cwd"]}) + "\n" + json.dumps({"type": "message", "id": "leaf1"}) + "\n")
        self.state = self.base / "state"
        self.env = dict(os.environ, REIGNITE_CLI=str(CLI), REIGNITE_STATE_DIR=str(self.state))
        self.sidecar = self.base / "assistant sessions.json"
        self.other = {"tool": "codex", "session_id": "untouched", "cli_args": "--foo", "env": {"keep": "fixture"}}
        self.request("open", ticket=None)
        self.request("enable")
        self.request("observe", activity="busy")

    def tearDown(self):
        self.temporary.cleanup()

    def request(self, op, **fields):
        request = {"op": op, "identity": self.identity, "pid": os.getpid(), **fields}
        result = subprocess.run([str(CLI), "--state-dir", str(self.state), "request"], input=json.dumps(request), text=True, capture_output=True)
        response = json.loads(result.stdout)
        self.assertEqual(result.returncode == 0, response["ok"])
        return response

    def hook(self, action):
        subprocess.run(["python3", str(HOOK), action, str(self.sidecar)], env=self.env, check=True, capture_output=True)
        return json.loads(self.sidecar.read_text())["sessions"]

    def old(self):
        path = self.state / "state.json"
        db = json.loads(path.read_text())
        for record in db["records"].values():
            record["owner"]["boot"] = "00000000-0000-4000-8000-000000000001"
        path.write_text(json.dumps(db))

    def test_exact_registration_ticket_reaches_restore_args_and_is_not_resaved(self):
        self.sidecar.write_text(json.dumps({"sessions": [self.other, {"tool": "pi", "pid": str(os.getpid()), "pane": "fixture:0.0", "session_id": "wrong-cwd-guess", "cwd": "wrong", "cli_args": "--model fixture --recovery-interactive --recovery-ticket stale --recovery-ticket=old --thinking off"}]}))
        saved = self.hook("reconcile")
        self.assertEqual(saved[0], self.other)
        self.assertEqual(saved[1]["session_id"], self.identity["session_id"])
        self.assertEqual(saved[1]["recovery_session_file"], str(self.session))
        self.assertEqual(saved[1]["cwd"], self.identity["cwd"])
        self.assertEqual(saved[1]["cli_args"], "--model fixture --recovery-interactive --thinking off")
        self.old()
        restored = self.hook("prepare")
        args = restored[1]["cli_args"].split()  # Same token contract as upstream.
        ticket = next(a.split("=", 1)[1] for a in args if a.startswith("--recovery-ticket="))
        self.assertEqual(restored[0], self.other)
        self.assertTrue(self.request("open", ticket=ticket)["ok"])
        self.assertFalse(self.request("open", ticket=ticket)["ok"])
        resaved = self.hook("reconcile")
        self.assertEqual(resaved[1]["cli_args"], "--model fixture --recovery-interactive --thinking off")
        # A stale snapshot may still contain the old ticket. Prepare strips it and
        # refuses another authorization; the CLI independently rejects direct replay.
        self.sidecar.write_text(json.dumps({"sessions": restored}))
        replay = self.hook("prepare")
        self.assertNotIn("--recovery-ticket", replay[1]["cli_args"])
        # Upstream whitespace splitting defines semantics, but untouched accepted
        # settings should also retain their original bytes when no ticket exists.
        replay[1]["cli_args"] = " --model   fixture  --recovery-interactive   --thinking off "
        self.sidecar.write_text(json.dumps({"sessions": replay}))
        self.assertEqual(self.hook("reconcile")[1]["cli_args"], replay[1]["cli_args"])

    def test_unsafe_legacy_pi_args_cannot_reach_launcher_but_original_rows_remain(self):
        unsafe = ["write a file", "@task.md", "-- task", "--extension", "--model=", "--model=fixture", "--thinking=off", "-e=adapter.ts", "--model --recovery-interactive", "--unknown-extension-flag task", "--print task", "--mode json", "--session-id other", "--recovery-ticket --model fixture"]
        launcher_output = self.base / "launcher.json"
        launcher = self.base / "launcher"
        launcher.write_text("#!/usr/bin/env python3\nimport pathlib,json\npathlib.Path(" + repr(str(launcher_output)) + ").write_text(json.dumps(json.loads(pathlib.Path(" + repr(str(self.sidecar)) + ").read_text())['sessions']))\n")
        launcher.chmod(0o700)
        for args in unsafe:
            with self.subTest(args=args):
                original = {"tool": "pi", "pid": str(os.getpid()), "pane": "fixture:0.0", "session_id": str(self.session), "cwd": self.identity["cwd"], "cli_args": args}
                self.sidecar.write_text(json.dumps({"sessions": [self.other, original]}))
                restored = subprocess.run(["bash", str(ROOT / "integration/restore.sh"), str(self.sidecar), str(launcher)], env=self.env, capture_output=True, text=True)
                self.assertEqual(restored.returncode, 0, restored.stderr)
                self.assertIn("skipped unsafe launch", restored.stderr)
                self.assertEqual(json.loads(launcher_output.read_text()), [self.other])
                data = json.loads(self.sidecar.read_text())
                self.assertEqual(data["recovery_skipped_pi"][0]["entry"], original)
                self.sidecar.write_text(json.dumps({"sessions": [self.other, original]}))
                self.assertEqual(self.hook("reconcile"), [self.other])

    def test_legacy_path_snapshot_migrates_without_weakening_ticket_identity(self):
        self.old()
        self.sidecar.write_text(json.dumps({"sessions": [{"tool": "pi", "pane": "fixture:0.0", "session_id": str(self.session), "cwd": self.identity["cwd"], "cli_args": "--recovery-interactive"}]}))
        restored = self.hook("prepare")[0]
        self.assertEqual(restored["session_id"], self.identity["session_id"])
        self.assertEqual(restored["recovery_session_file"], str(self.session))
        ticket = restored["cli_args"].split("--recovery-ticket=", 1)[1]
        self.assertTrue(self.request("open", ticket=ticket)["ok"])

    def test_native_id_and_exact_file_must_both_match_before_authorization(self):
        self.old()
        for session_id, file in [("wrong-id", str(self.session)), (self.identity["session_id"], str(self.base / "wrong.jsonl"))]:
            with self.subTest(session_id=session_id, file=file):
                self.sidecar.write_text(json.dumps({"sessions": [{"tool": "pi", "session_id": session_id, "recovery_session_file": file, "cwd": self.identity["cwd"], "cli_args": "--recovery-interactive"}]}))
                self.assertNotIn("--recovery-ticket", self.hook("prepare")[0]["cli_args"])
        records = json.loads((self.state / "state.json").read_text())["records"]
        self.assertTrue(all(record["attempt"] is None for record in records.values()))

    def test_sessionless_pi_relaunch_cannot_bypass_session_checks(self):
        other = {"tool": "codex", "pane": "fixture:0.1", "cmd": "codex"}
        pi = {"tool": "pi", "pane": "fixture:0.0", "cmd": "pi run this prompt", "cli_args": "--recovery-interactive"}
        for action in ["reconcile", "prepare"]:
            with self.subTest(action=action):
                self.sidecar.write_text(json.dumps({"sessions": [self.other], "relaunch": [other, pi]}))
                self.assertEqual(self.hook(action), [self.other])
                data = json.loads(self.sidecar.read_text())
                self.assertEqual(data["relaunch"], [other])
                self.assertEqual(data["recovery_skipped_pi"][0]["entry"], pi)
                self.assertEqual(data["recovery_skipped_pi"][0]["kind"], "relaunch")

    def test_heuristic_pi_entries_are_omitted_without_touching_other_harnesses(self):
        self.sidecar.write_text(json.dumps({"sessions": [self.other, {"tool": "pi", "pid": "999999999", "session_id": "guessed", "cli_args": "--recovery-ticket=stale"}]}))
        self.assertEqual(self.hook("reconcile"), [self.other])


if __name__ == "__main__":
    unittest.main()
