// Test-only human dialogs. Never installs into a user profile or starts model work.
import { appendFileSync } from "node:fs";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
export default function (pi: ExtensionAPI) {
  pi.on("session_start", (_event, ctx) => {
    // Do not await startup dialogs: the RPC input loop must remain available.
    const mark = (method: string) => (value: unknown) =>
      appendFileSync(process.env.PI_RPC_DIALOG_ANSWERS!, JSON.stringify({ method, value: value ?? null }) + "\n");
    void ctx.ui.select("Choose a branch", ["main", "experiment"]).then(mark("select"));
    void ctx.ui.confirm("Approve work?", "No approval has been given.").then(mark("confirm"));
    void ctx.ui.input("Which task?").then(mark("input"));
    void ctx.ui.editor("Clarify the task", "Waiting for the operator.").then(mark("editor"));
  });
}
