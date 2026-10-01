// Disposable smoke-only provider. No network, credentials, tools, or persisted prompts.
import { appendFileSync } from "node:fs";
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
export default function (pi: ExtensionAPI) {
  pi.registerProvider("recovery-smoke", {
    api: "recovery-smoke-api",
    baseUrl: "http://127.0.0.1/unused-no-network",
    apiKey: "local-fixture-not-a-secret",
    models: [{ id: "fixture", name: "Local fixture", reasoning: false, input: ["text"], cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }, contextWindow: 64000, maxTokens: 1000 }],
    streamSimple: (model, context, options) => {
      const stream = createAssistantMessageEventStream();
      const last = context.messages.findLast(m => m.role === "user");
      const text = typeof last?.content === "string" ? last.content : JSON.stringify(last?.content);
      const recovery = text?.includes("The host restarted") || false;
      appendFileSync(process.env.RECOVERY_SMOKE_OBSERVATIONS!, JSON.stringify({ recovery }) + "\n");
      const message: any = { role: "assistant", content: [], api: model.api, provider: model.provider, model: model.id, timestamp: Date.now(), usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } }, stopReason: "stop" };
      stream.push({ type: "start", partial: message });
      const finish = () => {
        message.content = [{ type: "text", text: "Local fixture completed." }];
        if (options?.signal?.aborted) { message.stopReason = "aborted"; message.errorMessage = "Cancelled fixture"; stream.push({ type: "error", reason: "aborted", error: message }); }
        else stream.push({ type: "done", reason: "stop", message });
        stream.end();
      };
      const requestedDelay = Number(process.env.RECOVERY_SMOKE_DELAY_MS);
      const delay = Number.isFinite(requestedDelay) && requestedDelay >= 30 && requestedDelay <= 60000
        ? requestedDelay : !recovery && text?.includes("busy fixture") ? 60000 : 30;
      const timer = setTimeout(finish, delay);
      options?.signal?.addEventListener("abort", () => { clearTimeout(timer); finish(); }, { once: true });
      return stream;
    },
  });
}
