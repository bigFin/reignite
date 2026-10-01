// Test-only provider: expose the interval between cancellation and its final message.
// No prompts, credentials or provider payloads are logged. Never installs into Pi.
import { appendFileSync } from "node:fs";
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

export default function (pi: ExtensionAPI) {
  // Optional test copy of Fabric's usage schema, driven by native message_end.
  // Like the existing tracker, it cannot log a still-unwinding cancellation.
  pi.on("message_end", (event, ctx) => {
    const path = process.env.PI_INTENT_USAGE_LOG;
    if (!path || event.message.role !== "assistant") return;
    appendFileSync(path, JSON.stringify({ schema_version: 1,
      timestamp: new Date().toISOString(), cwd: ctx.cwd,
      stop_reason: event.message.stopReason,
      attribution: { session_id: ctx.sessionManager.getSessionId(), source: "main" },
    }) + "\n");
  });
  pi.registerProvider("intent-fixture", {
    api: "intent-fixture-api",
    baseUrl: "http://127.0.0.1/unused-no-network",
    apiKey: "local-fixture-not-a-secret",
    models: [{ id: "fixture", name: "Offline cancellation fixture", reasoning: false,
      input: ["text"], cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
      contextWindow: 64000, maxTokens: 1000 }],
    streamSimple: (model, _context, options) => {
      const stream = createAssistantMessageEventStream();
      const mark = (event: string) => appendFileSync(process.env.PI_INTENT_EVENTS!,
        JSON.stringify({ event, pid: process.pid }) + "\n");
      const message: any = { role: "assistant", content: [], api: model.api,
        provider: model.provider, model: model.id, timestamp: Date.now(),
        usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0,
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
        stopReason: "pending" };
      mark("started");
      stream.push({ type: "start", partial: message });
      let finished = false;
      const finish = () => {
        if (finished) return;
        finished = true;
        if (options?.signal?.aborted) {
          message.stopReason = "aborted";
          message.errorMessage = "Cancelled offline fixture";
          stream.push({ type: "error", reason: "aborted", error: message });
        } else {
          message.stopReason = "stop";
          stream.push({ type: "done", reason: "stop", message });
        }
        stream.end();
      };
      let timer = setTimeout(finish, 60000);
      const abort = () => {
        mark("abort_seen");
        clearTimeout(timer);
        // A finite slow unwind, not an invented durable cancellation marker.
        const requestedDelay = Number(process.env.PI_INTENT_ABORT_DELAY_MS);
        const delay = Number.isFinite(requestedDelay) && requestedDelay >= 0 && requestedDelay <= 60000
          ? requestedDelay : 30;
        timer = setTimeout(finish, delay);
      };
      options?.signal?.addEventListener("abort", abort, { once: true });
      if (options?.signal?.aborted) abort();
      return stream;
    },
  });
}
