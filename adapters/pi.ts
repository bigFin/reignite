import { spawnSync } from "node:child_process";
import { existsSync, realpathSync } from "node:fs";
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";

/** Pi TUI only. Explicit flags, no global activation or transcript persistence. */
export default function recovery(pi: ExtensionAPI) {
  pi.registerFlag("recovery-interactive", { description: "Allow this top-level TUI recovery adapter", type: "boolean", default: false });
  pi.registerFlag("recovery-ticket", { description: "One-use restore authorization (not a saved setting)", type: "string" });
  let active = false;
  let broken = false;
  let prompts = 0;
  let suspended = false;
  let removeAbort: (() => void) | undefined;
  const cli = process.env.REIGNITE_CLI || "reignite";

  function identity(ctx: ExtensionContext) {
    const file = ctx.sessionManager.getSessionFile();
    if (!file || !existsSync(file)) throw new Error("Session is not yet persisted; send a first message before enabling recovery");
    return { harness: "pi", session_id: ctx.sessionManager.getSessionId(), session_file: realpathSync(file), cwd: realpathSync(ctx.cwd), leaf: ctx.sessionManager.getLeafId() };
  }
  function request(value: object): any {
    const args = process.env.REIGNITE_STATE_DIR ? ["--state-dir", process.env.REIGNITE_STATE_DIR, "request"] : ["request"];
    const result = spawnSync(cli, args, { input: JSON.stringify(value), encoding: "utf8", timeout: 5000, maxBuffer: 1024 * 1024 });
    if (result.error) throw result.error;
    const response = JSON.parse(result.stdout);
    if (result.status !== 0 || !response.ok) throw new Error(response.error || "Recovery CLI failed");
    return response.result;
  }
  function fail(ctx: ExtensionContext, error: unknown) {
    const wasOwner = active;
    broken = true;
    active = false;
    removeAbort?.();
    // A failed open never acquired the record. Other failures may disarm only
    // through the owner-checked protocol, not the operator's unrestricted disable.
    if (wasOwner) {
      try { request({ op: "observe", identity: identity(ctx), pid: process.pid, activity: "stopped" }); }
      catch { /* Original failure remains visible; durable disarm was not confirmed. */ }
    }
    ctx.ui.notify(`Recovery failed closed: ${String(error)}`, "error");
  }
  function observe(ctx: ExtensionContext, activity: "busy" | "idle" | "waiting" | "stopped") {
    if (!active || broken) return;
    try { request({ op: "observe", identity: identity(ctx), pid: process.pid, activity }); }
    catch (error) { fail(ctx, error); }
  }
  function work(ctx: ExtensionContext) {
    if (prompts === 0) { suspended = false; observe(ctx, "busy"); }
    removeAbort?.();
    const signal = ctx.signal;
    if (active && signal) {
      const abort = () => observe(ctx, "stopped");
      signal.addEventListener("abort", abort, { once: true });
      removeAbort = () => signal.removeEventListener("abort", abort);
      if (signal.aborted) abort();
    }
  }

  pi.on("session_start", (event, ctx) => {
    active = false; broken = false; prompts = 0; suspended = false; removeAbort?.();
    if (ctx.mode !== "tui" || pi.getFlag("recovery-interactive") !== true) return;
    const file = ctx.sessionManager.getSessionFile();
    if (!file || !existsSync(file)) {
      ctx.ui.notify("Recovery is off: persist a first message, then /recovery enable", "info");
      return;
    }
    try {
      const ticket = event.reason === "startup" ? pi.getFlag("recovery-ticket") : undefined;
      const result = request({ op: "open", identity: identity(ctx), pid: process.pid, ticket: typeof ticket === "string" ? ticket : null });
      active = true;
      if (result.attempt) {
        // The Rust transaction consumed the ticket before this supported API call.
        // A synchronous return means accepted by Pi, NOT model/tool delivery proof.
        pi.sendUserMessage(result.continuation);
        request({ op: "accepted", identity: identity(ctx), pid: process.pid, attempt: result.attempt });
        ctx.ui.notify("Recovery attempt accepted by Pi; inspect uncertain tool outcomes", "warning");
      } else ctx.ui.notify("Recovery is off; use /recovery enable for this session", "info");
    } catch (error) { fail(ctx, error); }
  });
  pi.registerCommand("recovery", {
    description: "Recovery enable | disable | status (top-level TUI only)",
    handler: async (args, ctx) => {
      if (ctx.mode !== "tui" || pi.getFlag("recovery-interactive") !== true) {
        ctx.ui.notify("Recovery requires an explicit --recovery-interactive top-level TUI", "error"); return;
      }
      try {
        if (args.trim() === "disable") {
          request({ op: "disable", session_file: identity(ctx).session_file });
          ctx.ui.notify("Recovery disabled durably", "info");
        } else if (args.trim() === "enable") {
          if (!ctx.isIdle() || ctx.hasPendingMessages()) throw new Error("Enable only when fully idle, with no queued work");
          if (!active || broken) request({ op: "open", identity: identity(ctx), pid: process.pid, ticket: null });
          request({ op: "enable", identity: identity(ctx), pid: process.pid });
          active = true; broken = false;
          ctx.ui.notify("Recovery enabled for this session; starts eligibility on actual work", "info");
        } else if (args.trim() === "status") {
          const records = request({ op: "status" }).records;
          ctx.ui.notify(JSON.stringify(records.find((r: any) => r.identity.session_file === identity(ctx).session_file) || null), "info");
        } else ctx.ui.notify("Use /recovery enable | disable | status", "info");
      } catch (error) { fail(ctx, error); }
    },
  });
  // agent_end is deliberately NOT idle: automatic retries/queues can follow.
  pi.on("agent_start", (_event, ctx) => { if (active) work(ctx); });
  pi.on("before_provider_request", (_event, ctx) => { if (active) work(ctx); });
  pi.on("tool_execution_start", (_event, ctx) => { if (active) work(ctx); });
  pi.on("turn_end", (event, ctx) => {
    if (event.outcome === "aborted") observe(ctx, "stopped");
    else if (prompts === 0 && !suspended) observe(ctx, "busy");
  });
  pi.on("message_start", (event, ctx) => {
    if (event.message.role === "assistant" && active) work(ctx);
  });
  pi.on("message_end", (event, ctx) => {
    if (event.message.role === "assistant" && event.message.stopReason === "aborted") observe(ctx, "stopped");
  });
  pi.on("agent_settled", (_event, ctx) => { observe(ctx, "idle"); removeAbort?.(); });
  pi.on("ui_prompt_start", (_event, ctx) => { prompts++; suspended = true; observe(ctx, "waiting"); });
  pi.on("ui_prompt_end", () => { prompts = Math.max(0, prompts - 1); });
  pi.on("session_before_switch", (_event, ctx) => { observe(ctx, "stopped"); });
  pi.on("session_before_fork", (_event, ctx) => { observe(ctx, "stopped"); });
  pi.on("session_before_tree", (_event, ctx) => { observe(ctx, "stopped"); });
  pi.on("session_tree", (_event, ctx) => { observe(ctx, "stopped"); });
  pi.on("session_shutdown", (event, ctx) => {
    removeAbort?.();
    if (active && !broken) {
      if (event.reason === "quit") {
        try { request({ op: "shutdown", identity: identity(ctx), pid: process.pid }); }
        catch (error) { fail(ctx, error); }
      } else observe(ctx, "stopped");
    }
    active = false;
  });
}
