# Transitional Pi/tmux prototype

This is the old extension-based delivery path, not the default standalone setup.
Native inspection and host policy are implemented; replacement automatic delivery
is not. See the [architecture](architecture.md), [policy](policy.md), and
[implementation plan](implementation-plan.md). Keep this path until replacement
acceptance passes, then remove it rather than maintaining a permanent legacy mode.

## Setup

Build the CLI and load the extension in a top-level interactive Pi session:

```sh
nix build
export REIGNITE_CLI="$PWD/result/bin/reignite"
# Optional: a private persistent directory, with an absolute canonical path.
export REIGNITE_STATE_DIR="$HOME/.local/state/reignite"
pi --extension "$PWD/adapters/pi.ts" --recovery-interactive
```

Send a first message to persist the session. Once Pi is idle, run
`/recovery enable`. Recovery is enabled for that session only.

| Command | Effect |
|---|---|
| `/recovery enable` | Enable while idle/stopped, clear the native disable, and manually reset the attempt; host policy must be enabled |
| `/recovery status` | Show this session's record |
| `/recovery disable` | Immediately save a disabled state |

New, ephemeral, and manually reopened sessions start with **legacy delivery** off.
This is separate from the new host-policy default. Print,
JSON, and RPC modes are excluded. Child agents and workflows with unobservable
approval state are unsupported. Keep this prototype out of global extensions.

The extension finds `reignite` on PATH by default. State goes under
`$XDG_STATE_HOME/reignite`, or `$HOME/.local/state/reignite` when unset.
`REIGNITE_CLI` and `REIGNITE_STATE_DIR` override those defaults. The CLI is
executed directly, not as a shell command. The state path must be canonical:
absolute, with no symlinks or traversal. State and Pi session files must survive
the restart; lost disks cannot be recovered.

## Recovery behavior

Rust records the native session ID, canonical file and working directory,
file device/inode, observed branch leaf, Linux boot ID, owner PID/start time,
activity, and recovery attempt. It does not choose conversations by timestamp.

1. The restore hook asks Rust for a ticket for an enabled session that was busy
   on a previous boot. Identity must match; ambiguous shutdown, a live current
   owner, and a previous attempt block recovery.
2. Rust issues one ticket for the exact record, branch, boot, and attempt. It
   expires after 120 seconds and reaches Pi as `--recovery-ticket=<uuid>`.
3. The extension durably claims it before calling `sendUserMessage`. The message
   instructs the agent to inspect interrupted work and uncertain tool outcomes,
   preserve approvals, and avoid blindly repeating side effects.

Host and native-session disable overrides also block authorization and ticket
claim. Policy toggles/clearing overrides never reset the legacy attempt. The
explicit old `/recovery enable` command still does; new sessions should use the
standalone tools instead of being enrolled into this prototype.

There is at most one automatic attempt per opt-in. A command may have succeeded
before the host died. A crash between claiming and sending can lose the recovery
message. Neither case is retried automatically.

`authorized`, `claimed`, and `accepted` remain visible across boots. `accepted`
means Pi's API returned, not that the provider received the message or any work
succeeded. Inspect and resume manually when needed. `/recovery enable` while
idle explicitly clears the old attempt.

### Blockers and observation limits

Idle, completed, stopped, disabled, and known native UI waits are ineligible.
Actual work is marked busy; `agent_settled` marks final idle state. `agent_end`
is not enough because retries or queued work may follow it.

Observed aborts disable recovery. Reloads, switches, forks, and tree navigation
do too; replacement sessions do not inherit opt-in. Native UI prompts block
recovery until actual work starts again, not merely until the dialog closes.
Approval is never inferred from conversation text.

Known gaps:

- Pi has no atomic public cancellation event. Observation can miss a
  cancellation, including during a retry gap.
- Native UI notifications arrive asynchronously. Saving state synchronously
  after notification cannot remove that delay. Custom overlays, external
  approvals, and unobserved input states are unsupported.
- Pi uses `session_shutdown: quit` for interactive quit, SIGTERM, and SIGHUP.
  Reignite records `shutdown_ambiguous` and blocks automatic recovery. An
  orderly host shutdown that emits this event needs manual recovery.
- Pi may append entries between observations. A crash in that gap can cause an
  exact branch-leaf check to refuse recovery.

## tmux companion hooks

The hooks wrap existing scripts without modifying the upstream plugin. Installing
them is a separate operator action; the repository changes no live configuration.

```sh
bash /path/to/reignite/integration/save.sh \
  /path/to/resurrect/assistant-sessions.json /path/to/existing-save-wrapper
# First select and copy the sidecar for the intended snapshot.
bash /path/to/reignite/integration/restore.sh \
  /path/to/resurrect/assistant-sessions.json /path/to/upstream-restore-wrapper
```

The restored pane's shell must find the executable and state directory.
Variables exported only inside a hook do not reach that shell.

Save runs upstream first, then reconciles Pi rows against exact registered live
processes, including boot/start time. It saves the native ID in `session_id`, the
canonical file in `recovery_session_file`, corrects cwd, and removes old tickets.
Unregistered or ambiguous Pi rows are omitted. Other harnesses remain unchanged.
Session-less Pi `relaunch` rows are skipped so they cannot bypass those checks.

The sidecar is the saved assistant-session JSON. If your wrapper copies one
beside each tmux snapshot, copy it after reconciliation. Restore must use the
sidecar for the selected snapshot, not a newer global copy.

Restore removes old tickets and requests a new one only for eligible Pi rows
saved with `--recovery-interactive`, with both native ID and exact file matching.
Old path-based Reignite rows are converted when they match a record. Rust
validates the actual file; upstream launches Pi by native ID. Pi resolves it in
the restored cwd/session directory, and the extension rejects a ticket if the
file, branch, or inode differs. Current upstream rejects absolute paths as IDs.

Failed authorization reopens without continuation. Duplicate launches, stale
snapshots, and expired tickets cannot reuse an attempt. Large restores can
exceed 120 seconds and require manual recovery.

### Launch arguments

Enter tasks interactively. The hooks accept a reviewed allowlist of interactive
options; prompts, `@file` inputs, `--` tails, noninteractive/session-override
flags, missing values, and unknown extension flags are skipped.

The tested Pi CLI takes separate built-in values: `--model name`, not
`--model=name`. Only the recovery ticket accepts the equals form here. Argument
text stays unchanged unless ticket removal requires rewriting its tokens.

Skipped rows remain in `recovery_skipped_pi`, outside executable arrays, with a
reason on stderr. They may contain prompts or credentials captured upstream.
Keep sidecars private and out of releases.

Retain `--extension` and `--recovery-interactive`. Upstream splits `cli_args` on
whitespace, so option values and extension paths cannot contain spaces. Cwd uses
a separately quoted field; session paths stay metadata and are resolved by ID.
Tickets belong on the launch command line, never in saved environment/settings.
The hooks treat upstream sidecars and launch arguments as trusted operator data.

## Retained adapter protocol

`reignite [--state-dir DIR] request` accepts one JSON object on stdin, up to
64 KiB. Unknown fields and operations are rejected. A legacy identity is:

```json
{"harness":"pi","session_id":"native-id","session_file":"/absolute/session.jsonl","cwd":"/absolute/project","leaf":"a1b2c3d4"}
```

The native version-3 header must match ID/cwd. `leaf` must exist or be JSON `null`.
Rust reads the real Linux boot ID; there is no CLI boot-override option. Legacy
validation now shares the inspector's stable-file checks and 128 MiB session /
1 MiB entry limits, rejects group/other-writable files and incomplete writes,
and checks ticket expiry after locking and source validation.

| `op` | Additional fields | Effect |
|---|---|---|
| `open` | `identity`, `pid`, `ticket` (string/null) | Register; manual reopen disarms; policy-gated valid ticket is claimed before returning the instruction |
| `enable` | `identity`, `pid` | Owned idle/stopped session; explicit legacy reset and override removal; host policy must allow it |
| `observe` | `identity`, `pid`, `activity` (`idle`, `busy`, `waiting`, `stopped`) | Owned observation; stopped adds a disable; never overwrite external disable |
| `shutdown` | `identity`, `pid` | Owned ambiguous-quit observation |
| `accepted` | `identity`, `pid`, `attempt` | Record API return, even after disable; not provider receipt or completion |
| `disable` | `session_file` | Native disable without enrollment; retain attempts and disarm matching old records |
| `status` | none | Retained records, boot, and policy |
| `recover` | `session_file`, `dry_run` (boolean) | Registered-session eligibility/ticket; unregistered native sessions stay ineligible |

The old extension calls the CLI synchronously; full-session validation can add
latency. Corrupt state, identity/branch/inode changes, and owner mismatch block
recovery. An owning extension tries to disable its record on failure; a rejected
duplicate must not disable another owner's session. Failed writes are visible,
not evidence that disable succeeded. State/migration rules are in [policy](policy.md).
