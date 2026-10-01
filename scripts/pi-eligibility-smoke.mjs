#!/usr/bin/env node
// Real supported SessionManager API: identical history cannot prove active branch
// or exclusive ownership. Disposable HOME/files, no provider or model work.
import assert from "node:assert/strict";
import { createHash, randomUUID } from "node:crypto";
import { chmodSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const packageDir = path.resolve(process.env.PI_PACKAGE_DIR);
const metadata = JSON.parse(readFileSync(path.join(packageDir, "package.json"), "utf8"));
const cli = path.resolve(process.env.CARGO_TARGET_DIR ?? path.join(root, "target"), "debug/reignite");
const base = mkdtempSync(path.join(tmpdir(), "reignite-pi-eligibility-"));
try {
  const home = path.join(base, "home");
  const project = path.join(base, "project with spaces");
  mkdirSync(home); mkdirSync(project);
  const env = { PATH: process.env.PATH, HOME: home, PI_OFFLINE: "1",
    PI_CODING_AGENT_DIR: path.join(home, ".pi/agent"),
    XDG_CONFIG_HOME: path.join(home, ".config"), XDG_DATA_HOME: path.join(home, ".local/share"),
    XDG_STATE_HOME: path.join(home, ".local/state"), XDG_CACHE_HOME: path.join(home, ".cache") };
  // Set the disposable profile before importing the supported SDK. No credentials.
  for (const key of Object.keys(process.env)) delete process.env[key];
  Object.assign(process.env, env);
  const { SessionManager } = await import(pathToFileURL(path.join(packageDir, "dist/index.js")).href);
  const session = path.join(base, "native-session.jsonl");
  const now = new Date().toISOString();
  const usage = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0,
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } };
  const nativeId = randomUUID();
  const entries = [
    { type: "session", version: 3, id: nativeId, cwd: project, timestamp: now },
    { type: "message", id: "user1", parentId: null, timestamp: now,
      message: { role: "user", content: "Wait for my branch choice", timestamp: 1 } },
    { type: "message", id: "answer1", parentId: "user1", timestamp: now,
      message: { role: "assistant", content: [{ type: "text", text: "Choose a branch before proceeding" }],
        api: "recovery-smoke-api", provider: "recovery-smoke", model: "fixture", usage, stopReason: "stop", timestamp: 2 } },
    { type: "session_info", id: "name1", parentId: "answer1", name: "fixture", timestamp: now },
  ];
  writeFileSync(session, entries.map(entry => JSON.stringify(entry) + "\n").join(""));
  chmodSync(session, 0o600);
  const digest = () => createHash("sha256").update(readFileSync(session)).digest("hex");
  const before = digest();
  function assess() {
    const result = spawnSync(cli, ["--state-dir", path.join(base, "state"), "assess-pi", "--session", session],
      { env, encoding: "utf8", timeout: 10_000 });
    assert.equal(result.status, 0, result.stdout);
    const report = JSON.parse(result.stdout).result;
    assert.equal(report.session.native_id, nativeId);
    assert.equal(report.eligible, false);
    assert.equal(report.decision, "observe_only");
    assert.equal(report.actions.submit_continuation, false);
    assert.equal(report.actions.acquire_execution_owner, false);
    for (const fact of ["authoritative_pre_crash_branch", "human_decision_state", "exclusive_execution_owner", "authorized_native_recovery_episode"]) {
      assert(report.requirements.some(item => item.fact === fact && item.state === "missing"));
    }
    assert.equal(report.persisted_lineage.last_message_role, "assistant");
    assert(report.reasons.some(item => item.code === "reported_assistant_turn_ended_or_waiting"));
    return report;
  }
  const baseline = assess();
  const first = SessionManager.open(session);
  const second = SessionManager.open(session);
  assert.equal(first.getSessionId(), nativeId);
  assert.equal(first.getLeafId(), "name1");
  assert.equal(second.getLeafId(), "name1");
  // Public API changes active context without a durable new leaf.
  first.branch("user1");
  assert.equal(first.getLeafId(), "user1");
  assert.equal(second.getLeafId(), "name1");
  assert.equal(first.buildSessionContext().messages.length, 1);
  assert.equal(second.buildSessionContext().messages.length, 2);
  assert.equal(digest(), before);
  assert.deepEqual(assess().requirements, baseline.requirements);
  assert.equal(SessionManager.open(session).getLeafId(), "name1");
  first.resetLeaf();
  assert.equal(first.getLeafId(), null);
  assert.equal(first.buildSessionContext().messages.length, 0);
  assert.equal(digest(), before);
  assert.deepEqual(assess().requirements, baseline.requirements);
  assert.equal(digest(), before);
  // Native writer creates a session in Pi's ordinary workspace-grouped storage.
  // These are explicit fixture messages, not model work or recovery authorization.
  const saved = SessionManager.create(project);
  saved.appendMessage({ role: "user", content: "Offline discovery fixture", timestamp: 3 });
  saved.appendMessage({ ...entries[2].message, stopReason: "aborted", timestamp: 4 });
  const savedFile = saved.getSessionFile();
  assert(savedFile);
  const savedBytes = readFileSync(savedFile);
  const discovered = spawnSync(cli, ["--state-dir", path.join(base, "state"), "discover", "--json"],
    { env, encoding: "utf8", timeout: 10_000 });
  assert.equal(discovered.status, 0, discovered.stdout);
  const inventory = JSON.parse(discovered.stdout).result;
  assert.equal(inventory.automatic_recovery_implemented, false);
  assert.equal(inventory.boot_change_verified, false);
  assert.equal(inventory.findings.length, 1);
  assert.equal(inventory.findings[0].native_id, saved.getSessionId());
  assert.equal(inventory.findings[0].category, "recorded_cancellation");
  assert.equal(inventory.findings[0].gate_decision, "blocked");
  const plain = spawnSync(cli, ["--state-dir", path.join(base, "state"), "discover"],
    { env, encoding: "utf8", timeout: 10_000 });
  assert.equal(plain.status, 0, plain.stderr);
  assert(plain.stdout.includes("Nothing was restarted"));
  assert(plain.stdout.includes("Saved history reports cancellation"));
  assert.deepEqual(readFileSync(savedFile), savedBytes);
  console.log(`PASS: Pi ${metadata.version} SDK branch/reset without persisted change, concurrent managers, native-written session discovered without enrollment and left unchanged; no model/reboot/ownership inference`);
} finally {
  rmSync(base, { recursive: true, force: true });
}
