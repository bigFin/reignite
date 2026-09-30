// Simulated Pi event dispatch; real adapter/CLI/persistence. Real Pi smoke is separate.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, readFileSync, rmSync, realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
import { spawnSync } from "node:child_process";
import recovery from "../adapters/pi.ts";

function fixture(mode = "tui", ticket?: string) {
  const dir = mkdtempSync(join(tmpdir(), "recovery-adapter-"));
  const file = join(dir, "session with spaces.jsonl");
  const cwd = realpathSync(process.cwd());
  writeFileSync(file, JSON.stringify({ type: "session", version: 3, id: "adapter-session", cwd }) + "\n" + JSON.stringify({ type: "message", id: "leaf1", message: { role: "user", content: "fixture" } }) + "\n");
  process.env.REIGNITE_CLI = resolve(process.env.CARGO_TARGET_DIR || "target", "debug/reignite");
  process.env.REIGNITE_STATE_DIR = join(dir, "state");
  const handlers = new Map<string, Function[]>();
  const commands = new Map<string, any>();
  const notices: string[] = [];
  const messages: string[] = [];
  let signal: AbortSignal | undefined;
  const ctx: any = { mode, cwd, ui: { notify: (text: string) => notices.push(text) }, sessionManager: { getSessionFile: () => file, getSessionId: () => "adapter-session", getLeafId: () => "leaf1" }, isIdle: () => true, hasPendingMessages: () => false, get signal() { return signal; } };
  const pi: any = { registerFlag() {}, getFlag: (name: string) => name === "recovery-interactive" ? true : ticket, on: (name: string, fn: Function) => handlers.set(name, [...(handlers.get(name) || []), fn]), registerCommand: (name: string, command: any) => commands.set(name, command), sendUserMessage: (text: string) => messages.push(text) };
  recovery(pi);
  const emit = async (type: string, fields: any = {}) => { for (const h of handlers.get(type) || []) await h({ type, ...fields }, ctx); };
  const command = (args: string) => commands.get("recovery").handler(args, ctx);
  const state = () => Object.values(JSON.parse(readFileSync(join(dir, "state/state.json"), "utf8")).records)[0] as any;
  return { dir, file, ctx, notices, messages, pi, emit, command, state, signal: (s: AbortSignal | undefined) => { signal = s; }, cleanup: () => { rmSync(dir, { recursive: true, force: true }); delete process.env.REIGNITE_CLI; delete process.env.REIGNITE_STATE_DIR; } };
}

test("native wait suspends until work, agent_end is not idle, abort and replacement disarm", async () => {
  const f = fixture();
  try {
    await f.emit("session_start", { reason: "startup" }); await f.command("enable");
    await f.emit("agent_start"); assert.equal(f.state().activity, "busy");
    await f.emit("agent_end"); assert.equal(f.state().activity, "busy");
    await f.emit("ui_prompt_start"); assert.equal(f.state().activity, "waiting");
    await f.emit("ui_prompt_end"); await f.emit("turn_end", { outcome: "completed" }); assert.equal(f.state().activity, "waiting");
    await f.emit("tool_execution_start"); assert.equal(f.state().activity, "busy");
    await f.emit("agent_settled"); assert.equal(f.state().activity, "idle");
    const controller = new AbortController(); f.signal(controller.signal); await f.emit("agent_start"); controller.abort();
    assert.equal(f.state().enabled, false); await f.emit("agent_start"); assert.equal(f.state().activity, "stopped");
    f.signal(undefined);
    for (const boundary of ["session_before_switch", "session_before_fork", "session_before_tree"]) {
      await f.emit("agent_settled"); await f.command("enable"); await f.emit("agent_start"); await f.emit(boundary); assert.equal(f.state().enabled, false, boundary);
    }
    await f.command("enable"); await f.emit("agent_start"); await f.emit("session_shutdown", { reason: "quit" });
    assert.equal(f.state().activity, "busy"); assert.equal(f.state().shutdown_ambiguous, true);
    await f.emit("session_start", { reason: "reload" }); assert.equal(f.state().enabled, false);
    assert.equal(f.messages.length, 0);
  } finally { f.cleanup(); }
});

test("headless excluded and external disable is not overwritten", async () => {
  for (const mode of ["print", "rpc", "json"]) {
    const f = fixture(mode);
    try { await f.emit("session_start", { reason: "startup" }); await f.command("enable"); await f.emit("agent_start"); assert.equal(f.messages.length, 0); assert.ok(f.notices.some(n => n.includes("TUI"))); } finally { f.cleanup(); }
  }
  const f = fixture();
  try {
    await f.emit("session_start", { reason: "startup" }); await f.command("enable"); await f.emit("agent_start");
    const disabled = spawnSync(process.env.REIGNITE_CLI!, ["--state-dir", process.env.REIGNITE_STATE_DIR!, "disable", "--session", f.file]); assert.equal(disabled.status, 0);
    await f.emit("before_provider_request"); assert.equal(f.state().enabled, false);
    await f.command("disable"); assert.ok(f.notices.includes("Recovery disabled durably"));
  } finally { f.cleanup(); }
});

test("startup sends one supported API continuation only after durable claim, errors stay visible", async () => {
  const f = fixture();
  try {
    await f.emit("session_start", { reason: "startup" }); await f.command("enable"); await f.emit("agent_start");
    const path = join(f.dir, "state/state.json"); const db = JSON.parse(readFileSync(path, "utf8"));
    for (const r of Object.values(db.records) as any[]) r.owner.boot = "00000000-0000-4000-8000-000000000001";
    writeFileSync(path, JSON.stringify(db));
    const authorization = spawnSync(process.env.REIGNITE_CLI!, ["--state-dir", process.env.REIGNITE_STATE_DIR!, "recover", "--session", f.file], { encoding: "utf8" }); assert.equal(authorization.status, 0);
    const ticket = JSON.parse(authorization.stdout).result.ticket;
    const handlers = new Map<string, Function[]>(); const pi = { ...f.pi, getFlag: (name: string) => name === "recovery-interactive" ? true : ticket, on: (name: string, fn: Function) => handlers.set(name, [...(handlers.get(name) || []), fn]), sendUserMessage: (text: string) => { assert.equal(f.state().attempt.status, "claimed"); f.messages.push(text); } };
    recovery(pi);
    for (const h of handlers.get("session_start")!) await h({ type: "session_start", reason: "startup" }, f.ctx);
    assert.equal(f.messages.length, 1); assert.equal(f.state().attempt.status, "accepted");
    const owningRecord = f.state();
    for (const h of handlers.get("session_start")!) await h({ type: "session_start", reason: "startup" }, f.ctx);
    assert.equal(f.messages.length, 1); assert.ok(f.notices.some(n => n.includes("failed closed")));
    assert.deepEqual(f.state(), owningRecord, "a rejected duplicate must not disarm the owning session");
  } finally { f.cleanup(); }
});
