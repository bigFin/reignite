# Native Pi eligibility assessment

`reignite assess-pi --session EXACT_FILE [--subagents-root EXACT_ROOT]` combines
bounded native inspection with the **existing** host policy and retained attempts.
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
- Rust CLI fixtures cover policy/alias/attempt retention, questions through
  metadata, branch separation, uncertain tools, opaque context, malformed graphs,
  bounded input and removed workspaces. Positive metadata is never promoted into
  a native authorization fixture.

## What comes next

A supported owner-bound native route must establish the missing facts before
any automatic continuation. Where it cannot, retain observation-only behavior
and let the normal operator client make the next explicit decision. Human routing,
episode/rearming policy and delegated recovery remain separate unfinished work.
Do not solve evidence gaps with mandatory extensions, upstream forks, a competing
lifecycle ledger, fabricated busy state or an optimistic history classifier.
The [legacy-ticket RPC transport](pi-rpc-transport.md) remains a separately scoped
experiment; its simulated-boot delivery acceptance does not satisfy this contract.
