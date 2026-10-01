# CLI reference

Successful JSON commands return `{"ok":true,"result":...}`. The interactive
`handoff-pi` exception inherits Pi's terminal output and exit status. Failures return
`{"ok":false,"error":"..."}` and exit 1. Argument/help handling uses clap's normal
stderr and exit behavior. `--state-dir` / `REIGNITE_STATE_DIR` selects policy state;
[pure inspection](#pi-native-inspection) does not create or modify that state.

## Pi native inspection

```sh
reignite inspect --session /absolute/canonical/session.jsonl
reignite inspect --session /absolute/canonical/session.jsonl \
  --subagents-root /absolute/canonical/runtime-root
```

No extension, enrollment, reconciliation, or agent launch. `--subagents-root`
defaults to `PI_SUBAGENTS_TEMP_ROOT`, reading `async-subagent-runs/*/status.json`
beneath that root, not discovering arbitrary storage.

The report includes native identity, last **persisted** entry (not authoritative
active branch), reported child IDs/states, provenance, coverage, and missing
evidence. `eligible` stays false. Parent matching uses the exact full session-file
path or native ID, never a basename/symlink alias; reported attribution is not
verified parent-branch ownership. Child coverage is never claimed complete.

Sources must be canonical user-owned regular files, not group/other writable.
Symlinks, unknown versions, and incomplete writes are rejected. Limits: 128 MiB
per session, 1 MiB per entry/status, 256 run directories, approximately 8 MiB of
status reads plus at most one record. Truncation/corrupt/missing data remain
visible. Prompts, tool arguments, and contracts are not returned. File/byte/time
metrics support measurement, not claims of a performance advantage.

## Native Pi eligibility assessment

```sh
reignite assess-pi --session /absolute/canonical/session.jsonl \
  --subagents-root /absolute/canonical/runtime-root
```

`--subagents-root` is optional and defaults to `PI_SUBAGENTS_TEMP_ROOT`. This
policy-aware report needs no enrollment, never launches Pi or issues a ticket,
and always returns `eligible: false`, `decision: "observe_only"`. Only
`actions.inspect` is allowed. It separates verified policy/selected-file facts,
retained attempts/blockers, raw persisted-lineage signals and missing authority.

Questions/ended turns remain negative signals even after name/label metadata;
abandoned branches are not merged. Tool pairing does not prove side effects.
Summaries, context edits and custom state are opaque, not eligibility markers.
Missing workspaces are reported without loading/repair. Strict ancestry and
65,536-entry/tool-reference caps supplement the inspector's byte limits.

Policy reads may create the directory/lock, but never rewrite `state.json`,
migrate state or change native/child records. The JSON request equivalent is
`assess_pi` with `session_file` and optional `subagents_root`. Read the
[evidence contract and native proofs](native-eligibility.md) before interpreting
reported history or the transitional transport as authorization.

## Read-only Codex probe

```sh
reignite probe-codex --socket /absolute/canonical/server.sock \
  --thread EXACT_NATIVE_THREAD_ID --protocol app-server-v2 --timeout-ms 5000
```

The explicit profile means **WebSocket over Unix** plus app-server v2 initialization,
`initialized`, and `thread/read` with `includeTurns: false`. It is not a generic
socket detector. Kitu's managed socket framing/additional-client compatibility
remains unverified; fixtures cannot establish it. Do not attach to another
client's private stdio.

The socket must be canonical, user-owned, and not group/other writable. Linux
peer credentials must match the user. This authenticates the connection, **not
exclusive thread ownership**. Thread identity must match exactly. Only selected
state fields are returned; unknown shapes stay unknown. No previews, turns,
server error messages, or tool arguments. `eligible` and
`execution_owner_verified` always stay false, including for idle.

No thread load/resume, work, approval answer, recovery state, or retry. Unexpected
server requests fail without a reply. Limits: 1–30,000 ms overall network deadline
(default 5,000), 128 KiB per frame/message, 1 MiB incoming wire bytes including
HTTP headers, and 64 messages. The deadline also bounds trickling notifications.
Reconnection repeats inspection only.

## Native recovery policy

```sh
reignite policy show [--session /absolute/canonical/session.jsonl]
reignite policy disable
reignite policy enable
reignite disable --session /absolute/canonical/session.jsonl
reignite policy clear-disable --session /absolute/canonical/session.jsonl
```

`show` reports default/overridden policy, optional native identity, and retained
legacy blockers/attempts; it does not authorize work. Disable needs no enrollment
and follows the native Pi ID across file moves. Enable does not erase overrides;
clearing an override does not reset attempts or arm a legacy session.
See [policy, trust, and migration](policy.md).

The JSON request interface also accepts `policy_status` (`session_file` string
or null), `set_host_policy` (`enabled` boolean), and `clear_disable`
(`session_file` string). These call the same policy operations, not a separate
runtime or lifecycle bridge.

## Manual operator handoff

```sh
reignite handoff-pi --session /absolute/canonical/session.jsonl \
  --pi-program /absolute/canonical/pi -- --provider ORIGINAL_PROVIDER --model ORIGINAL_MODEL
```

Requires terminal stdin/stdout/stderr. Validates the exact source and explicit
profile, refuses known retained live local owners, then replaces itself with the
ordinary Pi terminal client in the saved workspace. No prompt, continuation,
approval or ticket is sent. This is explicit manual use, not an automatic native
eligibility decision; ownership detection and lost branch/waits remain incomplete.

Native dialogs stay with the real operator client rather than Reignite disposing
an RPC child. It is **not** RPC-to-TUI attachment. Recovery disable and all attempts
are retained unchanged; ordinary explicit manual use does not re-enable recovery.
See [operator handoff and local crash evidence](operator-handoff.md).

## Experimental owned Pi transport

```sh
reignite deliver-pi --session /absolute/canonical/session.jsonl \
  --ticket EXISTING_LEGACY_TICKET --pi-program /absolute/canonical/pi \
  --timeout-ms 30000 -- --provider ORIGINAL_PROVIDER --model ORIGINAL_MODEL
```

This **launches Pi and can start model/tool work**. It is a bounded legacy-ticket
transport, not unattended native recovery. Select a trusted executable that
executes Pi directly, and supply the intended resource/permission profile after
`--`. No saved command or input replay; session/mode/positional overrides and
unreviewed profile flags fail before launch. Original-profile completeness and
exclusive conversation ownership are not established by the ticket.

Loading/inspection precede an atomic one-use claim and one fixed continuation.
Known questions, decisions, changed files/branches and uncertain tool history
hold delivery. No native UI response is sent. Without a human client, dialogs
hold and subprocess disposal may cancel them, not preserve a live waiting UI.
The old ticket does not settle unseen crash-time intent or human waits.

Reports distinguish observed API acceptance, settlement and completion (never
verified); native eligibility/exclusive conversation ownership always remain
unverified. Errors after claim leave delivery consumed/uncertain. No retry or
rearming. Bounds and safety limitations: [Pi RPC transport](pi-rpc-transport.md).

## Transitional delivery commands

`status`, `recover`, and the old adapter request operations remain until native
delivery passes replacement acceptance. `recover --dry-run` on an unenrolled
native session reports policy blockers/missing support and stays ineligible;
`recover` cannot deliver to it. Registered legacy sessions retain policy-gated,
120-second ticket behavior. Dry-runs explicitly label `eligibility_scope` as
`legacy_prototype` or `native_unimplemented`; `native_eligibility_verified` remains
false even when legacy conditions pass. Setup/protocol details are isolated in the
[transitional prototype](legacy-prototype.md), not the default native workflow.
