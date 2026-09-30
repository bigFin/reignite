# Reignite

Reignite helps a coding agent pick up interrupted work after a Linux host
restarts. tmux can reopen the terminal and conversation, but the agent may still
wait for another prompt. Reignite remembers which sessions were working and asks
eligible restored sessions to check what happened before continuing.

The Rust CLI stores recovery state and decides whether a session may continue.
A TypeScript extension delivers the recovery message to Pi.
[tmux-assistant-resurrect](https://github.com/timvw/tmux-assistant-resurrect)
still launches the sessions. Reignite has no daemon and does not change your
host configuration.

**Experimental, Linux-only.** Recovery is off unless you enable it for a
session. There is no exact Pi version requirement: compatibility depends on its
APIs and restore behavior, checked as described below. Other coding-agent
harnesses, child agents, and background workflows are unsupported. Full recovery
through a real host reboot has not been tested. Do not rely on this for
unattended production work.

## Try it with Pi

Build the CLI and load the extension in a top-level interactive Pi session:

```sh
nix build
export REIGNITE_CLI="$PWD/result/bin/reignite"
# Optional: use a private directory that survives host restarts.
export REIGNITE_STATE_DIR="$HOME/.local/state/reignite"
pi --extension "$PWD/adapters/pi.ts" --recovery-interactive
```

Send a first message so Pi creates its session file. Wait until Pi is idle, then
run `/recovery enable`. This enables recovery for that session only. New sessions
and sessions you reopen manually start with recovery off.

| Pi command | Purpose |
|---|---|
| `/recovery enable` | Enable recovery while idle; explicitly allow a fresh attempt |
| `/recovery status` | Show this session's recovery record |
| `/recovery disable` | Immediately save a disabled state |

Keep the extension out of Pi's global extensions directory. Print, JSON, and RPC
modes cannot use it, even with the flag. Do not enable it in child agents or in
workflows where it cannot see approval requests.

By default, the extension finds `reignite` on PATH and stores state in
`$XDG_STATE_HOME/reignite`, or `$HOME/.local/state/reignite` if that variable is
unset. `REIGNITE_CLI` and `REIGNITE_STATE_DIR` override those defaults. The state
path must be absolute and canonical: no symlinks or `..` components. The CLI
path is run directly, not interpreted as a shell command.

The state directory and Pi session files must survive the restart. Reignite
cannot recover files lost with the VM or its disk.

To connect automatic tmux restore, review and install the
[companion hooks](#connect-the-tmux-hooks). The restored pane's shell needs access
to the executable and state directory. Variables exported only inside a restore
hook do not reach that shell.

## How recovery works

Reignite records each session's native ID, canonical file path and working
directory, file device and inode, and current branch leaf (the last observed
session entry). It also records the Linux boot ID, owner process ID and start
time, activity, and any recovery attempt. It does not guess which conversation
to reopen from file timestamps.

When tmux restores a session:

1. The hook asks Rust for a ticket. The session must have been enabled and busy
   on a previous boot. Its identity must still match, with no ambiguous shutdown,
   live owner, or previous recovery attempt.
2. Rust issues a one-use ticket for that exact record, branch, boot, and attempt.
   It expires after 120 seconds. The hook passes it to Pi as
   `--recovery-ticket=<uuid>`.
3. The extension marks the ticket as claimed in the store before calling Pi's
   `sendUserMessage` API. The message tells the agent to inspect interrupted
   work, check uncertain tool outcomes, and preserve approval boundaries.

There is at most one automatic attempt per opt-in. This does **not** mean tools
run exactly once. A command may have succeeded before the host died. A crash
between claiming the ticket and sending the message can also lose the recovery
message. Reignite does not automatically retry either case.

The attempt states `authorized`, `claimed`, and `accepted` stay visible across
boots. `accepted` means Pi's API returned without an error. It does not prove
that the provider received the message or that any work succeeded. Inspect the
outcome and resume manually when needed. Running `/recovery enable` while idle
clears the old attempt and allows a fresh one.

### When recovery stays off

Idle, completed, stopped, disabled, and known native UI waits cannot trigger
recovery. The extension marks actual work as busy and waits for `agent_settled`
to mark it idle. `agent_end` is not enough: retries or queued work can follow it.

Observed aborts disable recovery. So do extension reloads, session switches,
forks, and tree navigation. A replacement session does not inherit the old
session's opt-in.

A native `ui_prompt_start` blocks recovery. Closing the dialog does not restore
eligibility; Pi must start actual work again. The extension does not infer
approval from conversation text.

Known limits:

- **Cancellation can race with observation.** Pi has no public event that
  atomically records user cancellation. The extension can miss cancellation
  before it receives a notification, including between retries.
- **UI notifications arrive asynchronously.** The extension saves state
  synchronously once notified, but cannot eliminate the delay before that.
  Custom overlays, external approvals, and input states it cannot observe are
  unsupported.
- **Quit and shutdown look alike.** Pi reports `session_shutdown: quit` for
  interactive quit, SIGTERM, and SIGHUP. Reignite keeps the busy record but marks
  `shutdown_ambiguous`, blocking automatic recovery. An orderly host shutdown
  that sends this event therefore needs manual recovery.
- **The branch must match exactly.** Pi may append session entries between
  observations. A crash in that gap can cause Reignite to refuse recovery.

## Connect the tmux hooks

`integration/save.sh` and `integration/restore.sh` wrap existing hooks without
modifying the upstream plugin. Installing them is a separate operator step.
Nothing here changes installed plugins or live configuration.

```sh
bash /path/to/reignite/integration/save.sh \
  /path/to/resurrect/assistant-sessions.json /path/to/existing-save-wrapper
# First select and copy the sidecar for the snapshot you intend to restore.
bash /path/to/reignite/integration/restore.sh \
  /path/to/resurrect/assistant-sessions.json /path/to/upstream-restore-wrapper
```

The save hook runs upstream first, then checks saved Pi rows against registered
live processes, including their boot and process start time. It writes the native
Pi ID into `session_id`, stores the exact canonical file separately in
`recovery_session_file`, corrects the working directory, and removes old tickets.
It omits unregistered or ambiguous Pi rows and leaves other harnesses unchanged.
Session-less Pi rows in upstream's `relaunch` array are also skipped: they cannot
bypass Reignite's session and argument checks.

The sidecar is the JSON file containing the saved assistant sessions. If your
wrapper keeps one beside each tmux snapshot, copy it **after** Reignite updates
it. Restore must use the sidecar for the selected snapshot, not a newer global
copy. Your existing wrapper still chooses the snapshot.

The restore hook removes old tickets. It requests a fresh one only for eligible
Pi rows saved with `--recovery-interactive`, with both the native ID and exact file
matching a record. Rust authorizes that file, not an ID guessed from a directory.
Old Reignite snapshots that stored the file in `session_id` are converted when
they match a record.

Upstream launches Pi with its native session ID. Pi resolves that ID in the
restored working directory and configured session directory; the extension checks
that it opened the exact recorded file before accepting the ticket. A different
file, branch, or inode blocks continuation. Current upstream rejects absolute
paths in `session_id`, so the path remains metadata rather than a launcher ID.

If authorization fails, the session opens without automatic continuation.
Duplicate launches and stale or expired tickets cannot reuse an attempt.
Restoring many panes may exceed the ticket's 120-second lifetime; those sessions
will need manual recovery.

### Supported launch arguments

Start Pi without a positional prompt or `@file`; enter tasks interactively.
Both hooks accept only a reviewed set of interactive options and their values.
They skip Pi rows containing prompts, `@file` inputs, `--` tails, noninteractive
or session-override flags, missing values, or unknown extension flags. Skipped
rows never reach the launcher. New flags need an allowlist review.

The tested Pi CLI takes separate values for built-in options: `--model name`,
not `--model=name`. Only the recovery ticket accepts the equals form here. Accepted
arguments keep their original text unless removing a ticket requires rewriting
them; rewriting preserves their whitespace-separated tokens.

Skipped rows remain under `recovery_skipped_pi`, outside the executable
`sessions` array, with a reason on stderr. They may contain prompts or
credentials captured by upstream. Keep sidecars private and out of releases.

Retain `--extension` and `--recovery-interactive` in saved launch arguments.
Upstream splits `cli_args` on whitespace and quotes each token before sending
the command to the pane. Option values containing spaces, including an
extension path, cannot be restored correctly. Use an extension path without
spaces. Working directories support spaces through a separate quoted field;
session file paths stay in metadata and Pi resolves them from the native ID.

Pass tickets on the launched Pi command line, never as saved environment
variables or ordinary settings. The hooks treat upstream sidecars and arguments
as trusted operator data.

## Develop and verify

The Nix flake supports `aarch64-linux` and `x86_64-linux`. It pins Rust,
formatting/lint tools, Node, TypeScript, Python, and tmux. Cargo dependencies are
locked. There is no rustup setup or network-install shell hook.

```sh
nix develop --command bash scripts/check.sh
```

This runs formatting, ShellCheck, workflow lint, Clippy, Rust tests, Pi extension
boundary tests, sidecar tests, and `nix flake check`. The last step builds and
tests the Rust package for the current architecture. For a smaller check:

```sh
nix develop --command cargo test --locked
nix build
```

In a Git checkout, add intended source files before checking the package: Nix
only sees tracked files. An extracted source archive also works. `cargo test`
builds the binary used by the extension and sidecar tests and respects
`CARGO_TARGET_DIR`.

Default tests use disposable files and processes. They simulate previous boots
by editing their own test records, and simulate Pi events against the real CLI
and store. They do not prove real Pi delivery or host reboot recovery.

For SDK typechecking and isolated tests with installed Pi and the upstream
plugin:

```sh
export PI_PACKAGE_DIR="$(npm root -g)/@earendil-works/pi-coding-agent"
export TMUX_ASSISTANT_PLUGIN_DIR="$HOME/.tmux/plugins/tmux-assistant-resurrect"
nix develop --command bash scripts/check.sh --integration
```

The Pi test runs its real terminal UI with a temporary home, project, and state
directory. It excludes discovered extensions, context files, and tools, and uses
a local fake provider: no network requests, model costs, or production
credentials. It simulates a boot change, checks that reopening by native ID
reaches the exact session file, checks one recovery message and final idle state,
and checks that duplicate and manual reopen attempts do not send another message. It kills only its own process group and saves no raw terminal
logs.

The tmux test uses the installed upstream restore script, a stub Pi executable,
and a private tmux socket and configuration. It stops only its own server. These
two tests check Pi delivery and tmux argument transport separately, not recovery
through a full reboot.

GitHub CI runs the default checks on both Linux architectures, with pinned
actions and read-only repository permissions. It caches Cargo downloads and
build outputs, not credentials or recovery state. A separate compatibility job
tests the current published Pi package and upstream tmux plugin on x86_64, on
changes and daily. It records their version and commit and uses disposable
fixtures, not production configuration. Check actual CI results; a local pass
does not establish that the CI jobs pass.

### Compatibility with fast-moving tools

Reignite does not compare Pi's version against a fixed allowed version. It needs
interactive mode detection, session identity APIs, `sendUserMessage`, and events
for final settlement, UI waits, aborts, and session replacement. It currently
reads Pi's version-3 session format. Unknown launch options are skipped rather
than guessed.

To check current upstream releases without updating your installed applications:

```sh
nix develop --command bash scripts/check-latest.sh
```

This downloads the latest published Pi package and a current tmux plugin checkout
into a temporary directory, then runs the default and integration checks. It
uses network access to fetch fixtures, but no production credentials or model
calls. To check your own installation, use `--integration` above instead.

Typechecking catches removed or changed declarations; the real smoke tests
check delivery, session resolution, settlement, and restore arguments. They do
not prove every lifecycle or approval behavior. Pi exposes no public list of
supported extension events, so registering a handler alone does not prove that
it will fire. Check compatibility after updates before enabling unattended
recovery. When a required contract changes, fix the adapter or hook and add a
regression test rather than freezing users on an old Pi version.

### Optional shell caching

[direnv with nix-direnv](https://github.com/nix-community/nix-direnv) can cache
shell setup. Install and hook it through your host configuration, review
`.envrc`, then run `direnv allow`. This repository does not install or authorize
it. With nix-direnv, `.envrc` disables fallback to an old shell so broken setup
remains visible. Plain `nix develop` works without direnv.

The flake is the only environment definition; there is no devenv configuration.
The Rust package includes only Cargo files, Rust source, and CLI tests, so changes
to documentation, the extension, or CI do not force a Rust package rebuild.
No project binary-cache uploads or extra trusted keys are configured.

## CLI and extension reference

Successful commands return `{"ok":true,"result":...}`. Failures return
`{"ok":false,"error":"..."}` and exit with status 1. clap handles argument and
help output with its usual stderr and exit behavior. The CLI authorizes
recovery; only the Pi extension sends the message to the agent.

```sh
reignite status
reignite disable --session /absolute/canonical/session.jsonl
reignite recover --session /absolute/canonical/session.jsonl --dry-run
# Authorize one restore attempt:
reignite recover --session /absolute/canonical/session.jsonl
# Assign result.ticket to the shell variable ticket, then pass it to Pi:
pi --extension /path/to/pi.ts --recovery-interactive \
  --session /absolute/canonical/session.jsonl --recovery-ticket="$ticket"
```

Extensions use `reignite [--state-dir DIR] request` with one JSON object on
stdin, up to 64 KiB. Unknown fields and operations are rejected. A session
identity looks like this:

```json
{"harness":"pi","session_id":"native-id","session_file":"/absolute/session.jsonl","cwd":"/absolute/project","leaf":"a1b2c3d4"}
```

The native version-3 session header must match the ID and working directory.
`leaf` must be an existing entry ID or JSON `null`.

| `op` | Additional fields | Effect |
|---|---|---|
| `open` | `identity`, `pid`, `ticket` (string or null) | Register the session; null disables recovery; a valid ticket is claimed before returning an instruction |
| `enable` | `identity`, `pid` | Enable recovery while idle/stopped and reset the attempt |
| `observe` | `identity`, `pid`, `activity` (`idle`, `busy`, `waiting`, `stopped`) | Check ownership and save activity; stopped disables recovery; observations cannot undo an external disable |
| `shutdown` | `identity`, `pid` | Check ownership and record an ambiguous quit |
| `accepted` | `identity`, `pid`, `attempt` | Record that Pi's API returned, without claiming execution |
| `disable` | `session_file` | Save a disabled state |
| `status` | none | Return records and the current kernel boot ID |
| `recover` | `session_file`, `dry_run` (boolean) | Explain eligibility or issue a one-use ticket |

Rust reads the kernel boot ID; there is no CLI option to spoof it. The library
exports `Store`, `Request`, and shared record types.

### Storage and trust

State belongs to one trusted Unix user. Processes running as that user can
change it, including extensions and agent tools. Reignite is not an
authorization sandbox. State and lock files reject symlinks, foreign ownership,
and permissions that allow group or other-user access. The state directory's
parents must also be trusted. Process start times distinguish ordinary PID
reuse; boot IDs distinguish a host restart from a process relaunch.

The versioned JSON store uses `flock` to prevent concurrent updates. Writes use
exclusive-create temporary files, file fsync, rename, and directory fsync. Only
local Linux filesystems are supported. There is no garbage collection, and reads
reject a store larger than 16 MiB.

Records contain metadata, not prompts, transcripts, tool arguments, secrets, or
commands to run. Validation reads native session files without copying them.
The extension calls the CLI synchronously to keep durable updates ordered.
Reading a large session on every observation can add latency.

Corrupt or unknown schemas, changed files, mismatched headers or working
directories, and ownership mismatches block recovery. Reignite does not silently
repair or reset them. Extension errors are visible and stop continuation. An
extension that owns the record tries to save a disabled state; a rejected
duplicate cannot disable another session's record. If storage is unavailable,
that write may fail too. Repair storage and inspect the record before enabling
recovery again.

## Before a stable release

Publishing experimental source is not the same as enabling unattended recovery.
Before a stable release or broad deployment, complete independent review,
clean-source builds, and a real reboot on a disposable host. Check approval,
cancellation, shutdown behavior, and restore of a specific tmux snapshot. Local
and isolated integration tests do not replace those deployment checks.

Keep credentials, state, session files, and logs out of published source.
Imported archives and local build/session data are excluded from Git. Cargo's
`publish = false` remains in place: this repository is not a crates.io release.

## License

[MIT](LICENSE). Dependency licenses still apply to their respective code.
