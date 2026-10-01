# Architecture

Reignite is a standalone recovery component, not a harness fork, agent server,
session manager, or workflow engine. It uses existing native APIs and records.
Host policy replaces routine per-session enrollment; explicit disable remains
durable. Pi is required support, not an optional follow-up to Codex.

This describes the target and its boundaries. Current implementation status is
in the [implementation plan](implementation-plan.md).

## Responsibility split

| Component | Owns |
|---|---|
| systemd or the existing supervisor | Backend process availability |
| Harness | Conversations, reconstruction, execution, tools, permissions, children |
| Reignite | Recovery policy, disable overrides, restore authorization, attempts |
| Existing operator client | Output, questions, choices, and approval answers |
| tmux, when used | Terminal restoration and presentation |

Restarting a process, reconnecting a client, or loading a conversation must not
start model work. A reboot does not answer an open question. A normally ended
assistant turn may be complete or waiting for input; neither idle state nor
question-mark matching establishes permission to continue.

## Operation-specific evidence

Inspection can be useful with incomplete evidence. Report provenance, coverage,
and unknowns rather than making inspection depend on every delivery guarantee.
Loading a conversation needs additional checks if it acquires or changes an
execution owner. Automatic continuation separately requires:

- Exact native identity, applicable workspace/branch, and execution scope.
- An authorized recovery episode after the relevant host restart.
- Sufficient authoritative lifecycle and human-wait evidence to preserve
  cancellation, unanswered decisions, approvals, and shutdown intent.
- Exclusive continuation ownership through the supported backend. Host boot,
  PID and process-start identity are one mechanism, not a universal server rule.
- Retained disable and attempt state, including uncertain delivery/tool outcomes.

Revalidate before delivery: another client, branch change, or changed file may
invalidate inspection. An external lock cannot fence a harness that ignores it.
Local reboot does not prove that remote work died. Unknown ownership or intent
leaves work waiting; there is no exactly-once claim or blind side-effect replay.

Select candidates from an explicit restore context, not every historical session
or every saved terminal pane. Compare performance only at equal evidence
coverage; neither a database query nor JSONL scanning is inherently faster.

## Native harness routes

### Pi

Pi owns version-3 JSONL history and conversation reconstruction. Its supported
SDK and stdin/stdout RPC need no Reignite extension. RPC controls an explicitly
owned subprocess: continuously consume stdout, correlate responses, preserve
resource/permission settings, and surface human dialogs through a real client.
Closed stdin shuts the subprocess down. It is not attachment to an existing TUI,
and TUI-only extension behavior must not silently disappear.

The standalone inspector reads existing history and reports the last persisted
entry. That is not universal pre-crash active-branch proof: native branch changes
can occur in memory without a new persisted entry. Native `parentId` expresses
conversation ancestry, not delegated-child ownership.

The [native assessor](native-eligibility.md) combines policy and retained attempts
with bounded raw ancestry; it permits observation only, not owner acquisition or
continuation. Real SDK tests demonstrate branch/reset changes without persisted
bytes and concurrent managers with differing contexts. Real RPC tests demonstrate
that idle/empty-queue state and unchanged history can coexist with human dialogs.
Those are evidence gaps, not permission to infer interrupted work.

The [bounded RPC transport](pi-rpc-transport.md) proves one fixed continuation
under existing legacy-ticket authority, without a Reignite extension or tmux.
Its simulated-boot/offline fixture is not native automatic eligibility. The
[explicit operator handoff](operator-handoff.md) chooses Pi's real terminal
client at launch; native dialogs remain open without a bounded RPC disposal.
This is ordinary manual use, not a live RPC-to-TUI transfer or positive recovery
decision. Live RPC human routing and automatic replacement remain unfinished.

The experimental upstream `@earendil-works/pi-server` is an optional deployment
candidate, not a prerequisite. It is a library, not a ready-made daemon. Any
adoption must prove durable history, existing-client/extension compatibility,
attach/detach, permissions, and subagent control. Its example in-memory repository
is not crash persistence. Do not build a second Pi server inside Reignite.

### Codex

App-server distinguishes `thread/read`, `thread/resume`, and `turn/start`.
Structured approval requests still belong to the operator client. The current
probe performs initialization and a selected read only; it never loads a thread,
starts work, subscribes broadly, or answers a server request.

A Unix socket's existence does not establish framing, additional-client support,
or execution ownership. On Kitu, the managed remote-control daemon/socket was
observed, but no live API handshake was tested. A separate t3code-owned server
uses private stdio; never attach to those pipes or load its active conversations
into another server by inference. Even successful inspection cannot prove that
no other server owns the thread. Bind future delivery to the actual owner.

### OpenCode and Google Antigravity

They are candidates, not implemented connectors. Verify their own native control,
identity, human-wait, and ownership contracts rather than treating the Codex
protocol as universal. Existing OpenCode remote-task supervision is harness-owned
behavior, not proof of host-wide Reignite recovery.

## Delegated work

Reuse pi-subagents' existing contracts, leases, worktrees, ownership checks, and
checked resume. Disk records are reported evidence, not complete child coverage.
Their normal parent reference is the full session-file path, with native ID
fallback; basename matching and conversation ancestry are insufficient.

Restore parent observation/result routing first. Leave surviving children alone,
consume completed results, and resume stopped work only through the existing
owner's permitted route. An idle parent with children is not automatically a
reason to start another parent turn. Never respawn an entire workflow.

A persistent runtime root does not establish retention completeness or fsync
crash durability. Missions are optional/best-effort. Missing contracts, removed
worktrees, foreign leases, or uncertain remote ownership block intervention;
Reignite must not repair or bypass those checks to make recovery appear possible.

## Cutover and deployment

Native [policy](policy.md) shares one locked state store with retained legacy
records; it does not duplicate conversations. Migration preserves old disables
and all attempts. Host-wide enablement is distinct from episode rearming.

Once replacement acceptance passes, remove the old adapter, lifecycle protocol,
enrollment flags, hook machinery, and prototype-only tests/docs together. Do not
maintain two permanent recovery runtimes. Until then, legacy delivery remains
explicitly [transitional](legacy-prototype.md).

Host installation, service changes, production restarts, and reboot verification
require separate approval. Prove the selected native route with a disposable,
tmux-free reboot first; test tmux separately only if deploying that transport.

## Native references

- [Pi RPC](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/rpc.md)
- [Experimental Pi server](https://github.com/earendil-works/pi/blob/main/packages/server/README.md)
- [Codex app-server](https://developers.openai.com/codex/app-server)
- [OpenCode server](https://opencode.ai/docs/server/)
