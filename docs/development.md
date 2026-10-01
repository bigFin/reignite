# Development and verification

The Nix flake supports `aarch64-linux` and `x86_64-linux`. Rust, Node, TypeScript,
Python, tmux, and lint tools are pinned; Cargo dependencies are locked. There is
no rustup setup or network-install shell hook.

## Local checks

```sh
nix develop --command bash scripts/check.sh
```

This runs formatting, ShellCheck, workflow lint, Clippy, Rust tests, simulated
Pi extension boundary tests, sidecar tests, and `nix flake check`. For a smaller
check:

```sh
nix develop --command cargo test --locked
nix build
```

In a Git checkout, Nix only sees tracked files; add intended source before
checking the package. Extracted source archives also work. `cargo test` builds
the binary used by adapter/sidecar tests and respects `CARGO_TARGET_DIR`.

Default tests use disposable files/processes and simulated boot changes. Legacy
regressions cover tickets expiring while waiting for the store lock and bounded,
complete, non-group/other-writable native sources with unchanged state on refusal. Native
policy tests cover unenrolled opt-out, native-ID/file moves, bounded header-only
reads, missing workspaces/incomplete tails, concurrent disables, late receipts,
strict schema validation, migration size failures, and unchanged attempts across
policy changes/migration. They do not launch a harness. Native inspection tests exercise the standalone CLI without enrollment or an extension,
including read-only behavior, incomplete writes, symlinks, and bounded discovery.
They do not prove automatic native-store eligibility, real Pi delivery, or host
reboot recovery. Codex CLI tests use disposable Unix-WebSocket protocol fixtures;
they cover initialization/read-only request logs, exact identity, reconnection,
approval requests without answers, unknown state, malformed/oversized responses,
timeout/trickling peers, disconnects, and unsafe endpoints. They do not establish
compatibility with a live Codex daemon or ownership across servers. Pi delivery
fixtures use a disposable Python RPC peer (a test-only package check dependency).
They cover one-use claims, identity/correlation/branch checks, dialog/question
holds, uncertain tools, source/policy/expiry changes, protocol budgets and lost
acknowledgements without retries. Their lifecycle authority is fabricated, not
proof of native automatic eligibility. Native assessment tests separately cover
strict raw ancestry, branch separation, questions through metadata, opaque context,
tool uncertainty, removed workspaces and unmodified policy/attempts across aliases
and schema-1 reads. All assert observation-only actions without harness launches. Manual handoff
fixtures use a PTY/native-client stand-in to check terminal-only execution, PID/
profile preservation, unchanged disable/attempt state and known live-owner refusal.

## Tmux-free native Pi checks

```sh
export PI_PACKAGE_DIR="$(npm root -g)/@earendil-works/pi-coding-agent"
nix develop --command bash scripts/check.sh --native-integration
# Or the individual native smokes after building the CLI:
nix develop --command python3 scripts/pi-rpc-smoke.py
nix develop --command node scripts/pi-eligibility-smoke.mjs
nix develop --command python3 scripts/pi-delivery-smoke.py
nix develop --command python3 scripts/pi-handoff-smoke.py
```

This opens a disposable native session three times through real Pi RPC with a
fresh HOME/profile and offline fixture provider. It checks exact file/ID/leaf,
unchanged history containing an unanswered conversational question, four native
UI dialog kinds left unanswered, zero model calls, and orderly stdin-EOF exit.
No Reignite extension, tmux, live session, provider credentials, or server service
is involved. Test-only extensions supply the offline provider and dialogs;
they are not product dependencies. Shutdown can cancel a pending dialog; that
is not an operator answer. RPC controls an explicitly owned subprocess, not an
existing TUI. This first smoke proves API building blocks, not automatic recovery.

The SDK smoke uses real supported `SessionManager` operations: two managers can
open the same native file, while `branch()`/`resetLeaf()` change active context
without persisted changes. Native assessment stays observation-only. The RPC
smoke also compares assessments with/without outstanding dialogs despite idle
state and an empty queue. No model work or ownership inference is made by these
[evidence tests](native-eligibility.md).

The delivery smoke exercises Reignite's [bounded owned transport](pi-rpc-transport.md)
through the real CLI: an offline original model turn is active, its exact owned
process is abruptly killed, and a simulated boot plus legacy ticket authorizes
one fixed continuation. It verifies native ID/file/leaf, acceptance/settlement,
duplicate suppression and unanswered question/dialog holds. Test-only private
snapshot rewinds are not product rearming. This is legacy-ticket transport
acceptance, not proof of native eligibility, exclusive conversation ownership,
a live RPC human-routing client, delegated recovery, or a full reboot. It now also
SIGKILLs the real controller/native child after API acceptance; consumed attempts
remain retained and duplicate delivery is blocked across another simulated boot.

The [handoff smoke](operator-handoff.md) uses real Pi's ordinary terminal client,
not an extension UI clone: sequential native dialogs stay open until explicit
fixture keystrokes. It kills the exact native PID at a wait and during active
explicit offline work, then reopens without continuation. Recovery disable stays
unchanged. This adds local process-loss evidence, not real host/Spot eviction,
fsync/boot ordering, remote survival or native automatic eligibility.

## Isolated integration checks

```sh
export PI_PACKAGE_DIR="$(npm root -g)/@earendil-works/pi-coding-agent"
export TMUX_ASSISTANT_PLUGIN_DIR="$HOME/.tmux/plugins/tmux-assistant-resurrect"
nix develop --command bash scripts/check.sh --integration
```

`--integration` also runs the tmux-free SDK, RPC and operator-handoff smokes. The Pi prototype test uses its real terminal UI, a temporary HOME/project/state, and a
local fake provider. Discovered extensions, context files, and tools are
excluded. There are no model/network calls or production credentials. It checks
native-ID reopening of the exact file, one recovery message, settlement, and
no repeated delivery on duplicate/manual reopen. It stops only its own process
group and saves no raw terminal logs.

The tmux test uses upstream restore, a stub Pi, and a private socket/config. It
stops only its own server. Pi delivery and tmux transport are tested separately;
neither test verifies a full reboot.

## Current upstream compatibility

```sh
nix develop --command bash scripts/check-latest.sh
```

This downloads the current published Pi package and tmux plugin into temporary
storage, records their version/commit, and runs default/integration checks.
Fetching fixtures requires network access; model calls and production
credentials are not used. Installed applications are unchanged.

GitHub default CI checks both Linux architectures with pinned actions and
read-only permissions. The separate compatibility job checks current Pi/tmux
on x86_64 on changes and daily. Caches contain Cargo downloads/build outputs,
not recovery state or credentials. Inspect actual job results; a local pass
is not a CI conclusion.

Compatibility follows APIs and behavior, not an exact-version allowlist. The
retained legacy adapter needs interactive mode, session identity, message delivery, and
settlement/UI/abort/session-replacement events. It reads Pi's version-3 JSONL.
Typechecking catches declaration changes; smoke tests catch exercised behavior.
Pi exposes no supported-event capability list, so registration alone cannot
prove an event fires. Fix broken contracts with regression tests rather than
requiring users to stay on an old release.

## Shell setup

Plain `nix develop` needs no direnv. For cached setup, install/hook nix-direnv
through host configuration, review `.envrc`, then run `direnv allow`. The repo
does not install or authorize it. With nix-direnv, fallback to an old shell is
disabled so broken setup stays visible.

The flake is the sole environment definition. The Rust package includes Cargo
files, Rust sources, and all Rust boundary tests, so documentation/adapter/CI edits do not force
a Rust package rebuild. No binary-cache uploads or trusted keys are configured.
