# Native Pi eligibility assessment

`reignite assess-pi --session EXACT_FILE [--subagents-root EXACT_ROOT]
[--usage-log EXISTING_LOG]` combines bounded native inspection with the
**existing** host policy and retained attempts. Its read-only
[cancellation gate](cancellation-gate.md) distinguishes recorded blockers from
uncertainty, optionally using existing Fabric Pi usage JSONL.
It needs no enrollment or Reignite extension and launches no harness. It never
issues/claims a ticket, answers a dialog, resumes a child, or feeds lifecycle state.

The current decision is always **`observe_only`**, with `eligible: false` and
only `actions.inspect: true`. This is not an unfinished positive heuristic:
the available records do not establish the required automatic authority. Enabling
host policy or finding a final user message does not fill those gaps. Ordinary
manual harness use remains outside these Reignite action permissions.

## What is known, reported, and missing

The report's `requirements` distinguish:

| Fact | Source and meaning |
|---|---|
| Host/session policy | Verified or blocked in the existing locked store; native-ID disables follow moved/copied files |
| Selected native file identity | Bounded canonical same-user version-3 header/history; identifies this selected artifact, not execution ownership |
| Workspace availability | Current canonical directory, or an explicit blocker; unavailable workspaces do not prevent reading the assessment |
| Retained attempts/blockers | Existing matching-native-ID records, including aliases; every authorized/claimed/accepted attempt stays retained |
| Pre-crash branch | Missing; persisted ancestry is not an authoritative active branch |
| Interrupted intent/cancellation | Missing; messages and legacy `busy` cannot reconstruct unseen crash-time intent |
| Human decision state | Missing; conversational questions and extension dialogs are different, and runtime dialogs need not be persisted |
| Exclusive execution owner | Missing; an old local PID or owned pipe cannot fence all other harness writers/remote owners |
| Original resource/permission profile | Missing; recorded system prompt/tool declarations are useful reported metadata, not complete implementations, trust settings, runtime arguments or permissions |
| Authorized native episode | Missing; policy enablement and an old ticket do not authorize a new native episode |
| Complete tool/child survival evidence | Missing; a persisted result or absent child record is not receipt, side-effect, retention or owner-death proof |

`reasons` identify policy and retained-state blockers separately from negative
history signals and missing authoritative evidence. Expired attempts, another
boot, copied histories and host toggles never reset delivery or rearm old work.
An accepted API call remains acceptance, not task completion. A reported live
retained owner blocks intervention but never proves exclusive ownership.

## Raw persisted lineage, not reconstructed execution

The assessor validates a bounded ID/parent graph and walks **only the ancestry
of the last physically persisted entry**. This handles multiple roots and avoids
merging abandoned branches. It reports the last conversational role/assistant
stop reason even when names, labels or system messages follow it. It does not
inspect text for question marks, commands, task completion or inferred approval.

The metadata projection counts unpaired tool calls/results and failed/unknown
results. Pairing alone does not attest provider receipt or actual tool effects.
Compactions, context edits, branch summaries, custom state/messages and unknown
execution roles are explicitly opaque. The assessor does not imitate Pi's model
context projection, interpret summaries as intent, or accept custom
`eligible`/`approval` markers as authority. It never returns prompts, declarations,
summaries, tool arguments/results, or custom payloads.

Missing/duplicate/forward/cyclic ancestry, malformed or incomplete JSONL, unsafe
sources, corrupt policy, and oversized input fail closed without repairing files
or replacing policy. Assessment adds caps of 65,536 entries and 65,536 tool
references to the inspector's existing 128 MiB session/1 MiB record bounds.
Files must remain unchanged by identity, size and modification/change timestamps,
including the pathname, while read. These are local snapshot checks, not a
continuation lease or a guarantee against later writes.

Policy reads may create the private state directory/lock, but never `state.json`,
perform schema migration or rewrite native/child records. Existing schema-1
policy/blockers are derived in memory and remain unchanged on disk.

## Verified native limitations

- `scripts/pi-eligibility-smoke.mjs` uses the real public Pi `SessionManager` API.
  `branch()` and `resetLeaf()` change in-memory active context without changing
  the file. Two managers can read different active contexts from identical
  retained bytes; reopening selects the persisted tip, not the lost selection.
  No model/provider or Reignite extension is involved. This is not proof that two
  live execution owners are legitimate—only that file opening does not fence them.
- `scripts/pi-rpc-smoke.py` opens real owned RPC subprocesses with and without
  outstanding `select`, `confirm`, `input` and `editor` requests. The same history,
  non-streaming/non-compacting state and empty message queue coexist with those
  waits. Native assessment remains observation-only with human-state evidence
  missing; no dialog response or model work is sent.
- `scripts/pi-intent-smoke.py` sends a real native `abort` while an offline
  provider is running. Its signal is cancelled, but during the provider's slow
  unwind, `get_state`, `get_entries`, `get_messages` and the session bytes remain
  identical to the active-work snapshot. Killing that exact PID before the final
  message loses the cancellation evidence. Reopening starts no model work and
  assessment stays observation-only. A separate control lets the abort finish
  and verifies Pi does persist the final `aborted` message normally.
- The same smoke also starts two real RPC processes on one disposable native
  session file. Both accept explicit test prompts and run model calls, with the
  same session ID/file and different in-memory leaves. There is no exclusive
  execution lease obtained by either native loading or prompting. These are
  offline fixture calls, not legitimate automatic recovery or permission to
  duplicate a live session. Test markers establish ground truth only; Reignite
  never reads them or receives fabricated lifecycle observations.
- Rust CLI fixtures cover policy/alias/attempt retention, questions through
  metadata, branch separation, uncertain tools, opaque context, malformed graphs,
  bounded input and removed workspaces. Positive metadata is never promoted into
  a native authorization fixture.

## Pi 1.0.0 verification

The same real offline tests passed against installed Pi 1.0.0 on 2026-10-01:

- Pending cancellation still leaves `get_state`, entries, messages, native bytes
  and the optional fixture usage log identical to active work. Exact-PID loss
  before the provider finishes still loses that cancellation evidence.
- Normal cancellation completion persists `aborted` and blocks the gate.
- Four native dialog kinds remain unanswered despite idle/empty-queue status;
  disposing them does not approve anything. TUI loss at a selector or active
  work and repeated reopening preserve the disable and start no recovery work.
- Two explicit fixture prompts still run concurrently against the same session
  file. Public SDK branch/reset still changes context without persisted changes.

Read-only inspection of this release's implementation agrees with the tests:
`AgentSession.abort()` sets an in-memory abort flag, cancels operations and waits
for idle; `Agent.abort()` signals its in-memory controller. Native message
persistence happens at `message_end`, after extension dispatch. This is not a
persisted pre-cancellation record. The ordinary CLI still uses version-3 history;
these results do not establish automatic recovery merely because another optional
harness API offers durable operation types.

Test cleanup targets tracked immediate subprocesses only, with PID/start checks,
not names or process groups. No live configuration, Kitu agent, provider account
or host reboot was involved. Pi 1.0.0 has not established a positive continuation
case under this project's ordinary-startup constraints.

## Current control-contract conclusion

The reviewed public Pi SDK/RPC can load a selected history and control a new
owned subprocess. It cannot attach to an existing TUI, acquire an exclusive
session-file execution lease, or reconstruct a cancellation/human wait lost
before persistence. A fresh RPC status describes the fresh process, not the
crashed one. Even a live `isStreaming: true` snapshot can coexist with a pending
abort. The persisted system loadout also does not reconstruct the complete
original launch environment or permission profile.

Under the current standalone/no-required-extension/no-competing-ledger
constraints, no positive automatic continuation case has been established for
ordinary existing Pi sessions. Do not implement one from an unfinished-looking
user message, a last observed busy flag, or possession of new pipes. Another
process-kill test, boot service, or VM cannot supply the missing contract.
Explicit normal TUI reopening remains available; the older ticket route remains
transport-only evidence. A supported owner-bound source of these facts is an
implementation prerequisite, not a deferred test assertion.

[Controlled-launch research](controlled-pi-launch.md) demonstrated one terminal
input ordering path, but its startup wrapper/key interception was rejected as a
product direction. It remains test evidence only. The current noninvasive work is
the [cancellation and uncertainty gate](cancellation-gate.md), not a custom host.

## What comes next

Keep normal harness startup unchanged. Use existing native records and any
already available, properly attributed telemetry to veto cancelled work and
expose uncertainty. Do not convert absent cancellation records or a shutdown
window into authorization. A supported owner-bound native route must establish the missing facts before
any automatic continuation. Where it cannot, retain observation-only behavior
and let the normal operator client make the next explicit decision. Human routing,
episode/rearming policy and delegated recovery remain separate unfinished work.
Do not solve evidence gaps with mandatory extensions, upstream forks, a competing
lifecycle ledger, fabricated busy state or an optimistic history classifier.
The [legacy-ticket RPC transport](pi-rpc-transport.md) remains a separately scoped
experiment; its simulated-boot delivery acceptance does not satisfy this contract.
