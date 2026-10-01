# Cancellation and uncertainty gate

`assess-pi` now separates **recorded blockers** from **uncertainty requiring an
operator decision**. It reads existing records only. It does not launch Pi,
change permissions, disable a session, issue a ticket, or implement automatic
recovery. Ordinary Pi startup and cancellation stay unchanged.

```sh
reignite assess-pi --session /absolute/canonical/session.jsonl
# Optional: an EXISTING Fabric Pi usage log, if this host already has one.
reignite assess-pi --session /absolute/canonical/session.jsonl \
  --usage-log /absolute/canonical/usage.jsonl
```

The top-level assessment remains `observe_only`, with `eligible: false`.
`recovery_gate.decision` is either:

- **`blocked`**: policy/retained attempts block recovery, or the selected raw
  ancestry reports cancellation, error, or an ended/waiting model turn.
- **`hold`**: no such scoped blocker was found, but interruption, cancellation,
  human waits, ownership, permissions or recovery authorization remain uncertain.

`automatic_candidate` is always false. A clean usage log, successful HTTP status,
empty queue or missing cancellation record does not authorize continuation.
These are assessment results, not new delivery enforcement or a boot service;
the separately scoped legacy-ticket transport has not been changed.

## Work-scoped native signals

`persisted_lineage.reported_work` describes the segment after the latest user
message **on the ancestry of the last physically persisted entry**. It reports
that user's entry ID, the last assistant entry/stop reason, and cancellation/error
counts. Metadata does not hide these signals; a later assistant response in the
same segment does not erase a recorded cancellation or error. Abandoned branches
are not merged.

This is a reported user-message segment, not proof of the actual pre-crash branch,
a new task, or an authorized recovery episode. A later user message starts a new
reported segment; it does not reset attempts, clear disables, or rearm anything.
An old cancellation is therefore not a permanent session disable. Missing facts
still put the newer segment on hold.

Pi's `stop`/`length` reasons are model-message boundaries, not proof that the task
is finished or that no human decision is pending. The gate blocks rather than
answering a possible question or sending a speculative follow-up. Prompts and
question marks are never interpreted as intent.

## Optional usage evidence

The optional reader supports the existing **Fabric Pi usage JSONL schema 1**.
This is an existing harness integration, not stock Pi storage and not a new
required Reignite extension. Nothing installs or enables the tracker. No default
log path is guessed; select the host's actual configured file explicitly.

The reader checks native session ID, `main`/`child` attribution and, for main
records, the saved workspace. It uses `stop_reason`, never HTTP success as proof
of completion. It returns counts and source coverage, not provider payloads,
prompts, arbitrary metadata or other sessions' identifiers.

These records lack native message-entry IDs, branch IDs and recovery-episode IDs.
Even a matching main cancellation is therefore an **unscoped session signal**:
`unscoped_main_cancellation_reported`, with the gate on hold unless native history
or policy also supplies a scoped blocker. It must not be assigned to the current
work using timestamps alone, nor treated as an irreversible session disable.

Child accounting uses a parent's session ID. A cancelled child is not evidence
that its parent was cancelled, that the child's owner died, or that the child
may be resumed. Other sessions' records are excluded. Unknown schemas, missing
attribution, identity conflicts and unknown stop reasons remain visible.

Bounds: 32 MiB per log, 64 KiB per record, 65,536 records. The existing canonical,
same-user regular-file checks reject symlinks and group/other-writable sources.
Complete lines and unchanged file/path identity, size and nanosecond timestamps
are required. Missing selected files, malformed JSON, incomplete writes and
exceeded limits fail closed without repair or state mutation. Separate file
checks do not make the sources an atomic crash-time snapshot.

## What the source research established

Read-only source inspection on 2026-10-01 found:

- Fabric's `harness/pi/extensions/usage-tracker.ts` writes usage on native
  `message_end`. `usage-record.ts` retains stop reason and session attribution;
  `usage-log.ts` synchronously appends JSONL without an explicit fsync.
- Installed Pi 0.99.2 dispatches that extension event before appending the native
  message. Usage can therefore provide an additional negative signal if loss
  occurs between those writes. This is not full-reboot durability proof.
- Both paths still wait for the provider's final message. Neither captures an
  abort still unwinding before `message_end`. The native process-loss smoke
  confirms that such an interval has no usage record and must stay on hold.
- Fabric's current SQLite mirror does not pass session ID or original request
  timestamps, and derives `ok`/`error` from HTTP status. Its metadata contains a
  stop reason, but those rows cannot safely identify cancelled work. Reignite
  does not read that database or modify its writer.
- The provider-router span schema lacks native session/turn attribution; the Pi
  LLMTrace routing header defaults to the generic agent ID `pi`. Generic gateway
  disconnects do not establish user cancellation or task completion.
- Current [Codex rollout policy](https://github.com/openai/codex/blob/main/codex-rs/rollout/src/policy.rs)
  persists `TurnStarted`, `TurnComplete` and `TurnAborted` events. Its
  [protocol](https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs)
  carries turn IDs/abort reasons. The
  [recorder](https://github.com/openai/codex/blob/main/codex-rs/rollout/src/recorder.rs)
  buffers writes, so absence of an abort is still not permission. A Codex
  cancellation reader and installed-release/owner checks remain unimplemented.

Checkout source is not evidence that the same integration is deployed on Kitu.
No live database, private conversations, host settings or Kitu agents were changed.

## Verification and remaining boundary

Rust CLI fixtures cover scoped cancellations through metadata and later assistant
messages, later user segments without rearming, abandoned branches, HTTP-success
cancellations, child attribution, unrelated sessions, unknown formats, identity
conflicts, corrupt/incomplete/unsafe sources and limits. They assert unchanged
native/log/policy bytes and no work-authorizing actions.

The real offline Pi intent smoke records the optional fixture usage schema from
native `message_end`, not from the provider's test markers. A pending abort leaves
both history and usage unchanged and the gate on hold. A normally completed abort
produces cancellation evidence and a blocked gate. Exact-PID loss is not a host
reboot or disk-crash test; this fixture is not the deployed Fabric tracker.

A timer around shutdown may add caution, but cannot distinguish two otherwise
identical saved states. Automatic recovery needs an agreed, evidenced positive
case; this change implements the negative/uncertainty gate, not that missing case.
The SDK startup/key-interception route was rejected and is not the product plan.
