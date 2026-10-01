# Experimental owned Pi RPC transport

`deliver-pi` is a bounded transport step, **not standalone automatic recovery**.
It uses an already authorized legacy restore ticket in the existing state store.
No Reignite extension or tmux is required by the transport. Unregistered native
sessions cannot obtain a ticket or use this route. The old lifecycle authority
remains transitional; this does not complete replacement acceptance.

Do not deploy this as unattended native recovery. A legacy ticket cannot prove
unseen crash-time intent, ephemeral human waits, exclusive conversation ownership,
remote-child death, or the original resource/permission profile. Native eligibility
and exclusive conversation ownership remain explicitly unverified in every report.

## Operation

1. Read host/session policy and the exact registered ticket scope. Require the
   old busy/enabled state, changed boot, no live old local owner, no ambiguous
   shutdown, and an unexpired, unconsumed ticket for this boot.
2. Inspect the exact native file/ID/cwd/device/inode and persisted leaf. Hold on
   a changed/unknown leaf, completed assistant turn (including questions without
   a question mark), or reported cancellation/error. These are negative guards,
   not a complete native eligibility classifier.
3. Launch the selected trusted executable in the saved workspace using private
   stdin/stdout pipes. Append only `--mode rpc --session EXACT_FILE`. Forward the
   explicit supported profile options and inherit the caller environment; never
   replay saved argv, prompts, commands, `@file` inputs or workflow scripts.
4. Separately request `get_state`, cursor-based `get_entries`, and `get_messages`.
   Verify exact native ID/file/leaf, idle/non-compacting state and an empty queue.
   Hold on startup activity, decisions, uncertain tool outcomes or file changes.
5. Atomically claim the existing ticket, binding the immediate owned PID and
   rechecking policy/identity/expiry through the same store as the old transport.
   Recheck policy/file stability, then send exactly one fixed continuation.
6. Record API acceptance only after a correlated successful `prompt` response.
   Wait for `agent_settled`, not merely `agent_end`; verify final native identity
   and idle state. Settlement is **not** proof of completed work or provider receipt.

Claiming precedes prompt bytes. Any subsequent loss, malformed acknowledgement,
held dialog, timeout or failure leaves the attempt consumed/uncertain. No retry,
rearming, cancellation reset or synthetic lifecycle feed is introduced. A late
acceptance receipt remains governed by the existing store contract.

## Profile and dialogs

The selected executable must be canonical, executable, owned by the caller or
root, and not group/other writable. It should execute Pi directly (wrappers must
use `exec`). This is a trusted user-selected program, not a sandbox. Startup
extensions can execute code; loading is not a read-only operation.

The caller must supply the intended resource, provider, model and permission
profile. Reignite does not reconstruct missing launch intent or add approval/
permission overrides. Only reviewed built-in profile switches and their values
are forwarded; positional inputs, session/mode overrides, unknown extension flags,
and restore-extension flags are rejected rather than guessed or stripped.
Credentials in supplied arguments or environment are not persisted by Reignite.

This bounded route has **no human client yet**. Native `select`, `confirm`, `input`
and `editor` requests cause a held result, never an `extension_ui_response`.
The owned subprocess is disposed on return: EOF may cancel pending native dialogs
(null/false), but Reignite never selects, supplies text, or approves. This does
not preserve a live waiting dialog or attach a terminal to an existing TUI.
An operator-facing routing client is required before automatic replacement.

Only the immediate owned child is signalled/reaped. No process-group signal,
workflow replay, child-resume operation or claim of exhaustive child coverage.
Original/surviving harness children remain outside this transport's authority.

## Bounds and evidence

The 1–120,000 ms overall subprocess-I/O deadline defaults to 30,000 ms. Pipe
consumption is continuous during requests, streaming and stdin backpressure;
stderr is drained but never printed/persisted. Limits: 1 MiB per JSONL record,
8 MiB stdout, 256 KiB stderr, 8,192 records and eight queued responses. Framing
uses LF only (optional CR stripped); Unicode separators remain JSON content.
Disposal adds at most a 500 ms grace period before killing/reaping the immediate
child. Local trusted filesystem/store operations use their existing bounds.

Rust protocol fixtures exercise identity/correlation/branch checks, startup waits,
file changes, policy/expiry changes, malformed/oversized/trickling peers, fixed
one-use delivery, loss of acknowledgement and post-submission uncertainty.
`scripts/pi-delivery-smoke.py` exercises the actual CLI and Pi RPC with an offline
provider and a real active original subprocess. **Its boot change and legacy
permission enrollment are fixture setup**, not evidence of standalone native
eligibility. It verifies one continuation/receipt, duplicate suppression and
question/dialog holds, without tmux or a Reignite extension. No full reboot,
production histories, live deployment, child recovery or network/model costs.
