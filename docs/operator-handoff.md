# Pi operator handoff and local crash tests

`handoff-pi` is an **explicit manual launch** of Pi's ordinary interactive client.
It is not automatic recovery or a positive native eligibility decision. It needs
no enrollment, ticket, Reignite extension, new UI or tmux.

```sh
reignite handoff-pi --session /absolute/canonical/session.jsonl \
  --pi-program /absolute/canonical/pi -- \
  --provider ORIGINAL_PROVIDER --model ORIGINAL_MODEL
```

Use a trusted executable/profile that preserves the intended resources and
permissions. All three standard streams must be terminals. Session/mode overrides,
positional prompts, `@file` inputs, restore-extension flags and unreviewed profile
arguments are rejected. Full bounded native inspection precedes launch; unsafe or
incomplete input is not handed to a loader for silent repair.

The command checks matching-native-ID retained records for a known live local
owner and refuses a second launch when one is found. **That is not exhaustive
ownership detection.** Use an existing client if another local/remote owner
survives; unrecorded owners and lost in-memory branches remain unverified.

After printing the limitations, Reignite uses Unix `exec` to replace itself with
Pi in the saved workspace, forwarding the explicit profile and exact session
file. The PID and attached terminal are preserved. No recovery message, tool
command or approval is sent. Pi's real client owns its dialogs, permissions,
editor, extension behavior and lifecycle. There is no 30-second delivery deadline
or Reignite stdin-EOF disposal while the user is considering a question.

This route is deliberately chosen **at launch**. Pi offers no supported TUI attach
to an existing owned RPC subprocess. It does not transfer a live RPC dialog into a
new TUI, reconstruct a pre-crash ephemeral callback, or pretend to preserve a lost
active branch. The [bounded legacy-ticket RPC route](pi-rpc-transport.md) remains
separate and still holds/disposes dialogs without an operator client.

Manual ordinary harness use is allowed even when **recovery** is disabled. This
command never clears that disable, claims/resets attempts, migrates state, arms
an episode or adds lifecycle observations. Policy reads may create the private
directory/lock. Existing state stays byte-for-byte unchanged by handoff itself.
Work can start when the operator gives Pi a new explicit instruction; trusted
startup extensions can also execute code, so loading is not inherently read-only.

Successful handoff inherits Pi's terminal output/exit behavior, **not JSON**.
Pre-launch Reignite failures keep the normal JSON error convention. There is no
wrapper supervisor silently restarting the client after exit or SIGKILL.

## Local evidence before Spot-instance testing

`scripts/pi-handoff-smoke.py` runs real Pi in private pseudo-terminals with a
fresh HOME/project/native session and an offline provider. Test-only extensions
provide the provider and sequential native select/confirm/input/editor dialogs;
none installs into the user's harness or supplies recovery authority.

It verifies:

- Exec reaches the actual Pi PID and native `tui` mode with the selected file.
- An unanswered conversational question starts no work.
- Startup dialogs stay unresolved until explicit fixture keyboard input; selection,
  a negative confirmation, text input and editor cancellation reach Pi normally.
- SIGKILL of precisely the owned native PID while waiting makes no choice/approval
  and starts no model call. The source history remains unchanged.
- Explicit offline fixture work starts once; killing its active PID and reopening
  the exact native file starts no continuation or additional model call.
- Repeated reopening leaves recovery disable unchanged. The Rust PTY fixtures also
  retain every authorized/claimed/accepted attempt and ambiguous/stopped state.

`scripts/pi-delivery-smoke.py` separately kills a real RPC controller and its
owned native child **after API acceptance**. The direct native child receives a
parent-death signal on controller loss; no group/foreign-child signals are used.
Consumed acceptance stays retained and duplicate delivery is blocked even across
another simulated boot. This does not claim completed execution or exactly-once
side effects.

All fixtures clean up only their processes/files, use no provider credentials or
network/model costs, and make no live service changes. A pseudo-terminal is
transport, not a fake positive native eligibility case. Test snapshot rewinds and
boot edits are confined to disposable fixture state, never product rearming.

## What PID kills cannot prove

SIGKILL tests loss of runtime memory, control pipes and process lifecycle. It does
**not** reboot the kernel, exercise real boot ordering/systemd activation, evict a
Spot VM, demonstrate crash persistence/fsync on the deployed disks, kill remote
owners/children, or prove original intent/permissions/branch/exclusive ownership.
Native assessment therefore remains observation-only.

Finish local checks before a scoped commit/push. Then obtain separate deployment
and disposable Spot/reboot-test scope: selected machine/session/profile/runtime
storage, expected owners and cleanup, service activation, and rollback for schema
2. Commit/push is not permission to install services or reboot. The full-reboot
core acceptance must remain tmux-free; delegated survival and optional tmux need
their own applicable checks. Remote CI outcomes must be inspected after publishing.
