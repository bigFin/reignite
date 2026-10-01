// Disposable native TUI dialogs. No Reignite extension/lifecycle feed/model work.
import { appendFileSync } from "node:fs";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
export default function (pi: ExtensionAPI) {
  pi.on("session_start", async (_event, ctx) => {
    const mark = (event: string, value?: unknown) =>
      appendFileSync(process.env.PI_HANDOFF_EVENTS!, JSON.stringify({ event, value: value ?? null, pid: process.pid, mode: ctx.mode }) + "\n");
    mark("select_waiting");
    mark("select_answer", await ctx.ui.select("HANDOFF_SELECT: choose a branch", ["main", "experiment"]));
    mark("confirm_waiting");
    mark("confirm_answer", await ctx.ui.confirm("HANDOFF_CONFIRM: approve fixture work?", "Nothing has been approved automatically."));
    mark("input_waiting");
    mark("input_answer", await ctx.ui.input("HANDOFF_INPUT: which task?"));
    mark("editor_waiting");
    mark("editor_answer", await ctx.ui.editor("HANDOFF_EDITOR: clarify task", ""));
    mark("ready");
  });
}
