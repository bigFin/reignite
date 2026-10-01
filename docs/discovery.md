# Finding saved work after a restart

Run `reignite discover` to find saved Pi sessions and get a plain-English report.
You do not need to register individual sessions or change how you start Pi.

The report separates:

- **Recorded cancellation:** saved history reports cancellation; do not
  automatically restart that recorded work.
- **Other recorded blockers:** recovery is disabled, previous attempts remain,
  a saved error needs a decision, or the response ended/may be waiting for you.
- **Needs your review:** the records do not establish safe interrupted work.
- **Cannot safely read:** damaged, unstable, unsafe or over-budget files remain
  visible, rather than being repaired or silently treated as recoverable.

Nothing is restarted, no question is answered, and no recovery attempt is issued
or reset. A cancelled user-message segment is not a permanent session disable.
The [cancellation gate](cancellation-gate.md) supplies these negative checks;
missing information is never permission to continue.

## Where it looks

Discovery covers the **current user's** ordinary Pi session storage across
workspaces: `~/.pi/agent/sessions`, or `PI_CODING_AGENT_DIR/sessions` when configured.
An existing `PI_CODING_AGENT_SESSION_DIR` is also searched. Environment paths may
use `~` or be relative to the command's current working directory.

For other known storage locations, supply directories—not individual enrollment:

```sh
reignite discover --sessions-root /absolute/canonical/saved-sessions
# More than one location:
reignite discover --sessions-root /absolute/location-a \
  --sessions-root /absolute/location-b
```

Explicit roots replace the default selection. Pi's project/global `sessionDir`
settings and past `--session-dir` arguments are not loaded or guessed. The report
always labels its coverage as limited to selected roots; it does not scan every
account, whole disks, arbitrary workspaces or remote child-agent storage.

Nested `.jsonl` files are considered, including native sessions in deeper run
folders. This does not prove a parent/child relationship or authorize child
recovery. Multiple files sharing one native ID are marked as aliases, not
independent jobs or recovery permissions. No newest-file or filename-pattern
heuristic chooses work to restart.

## Safety limits and policy

Directory traversal rejects symlinks and noncanonical, foreign-owned or
other-writable directories. Files use the existing stable, bounded native reader;
missing workspaces can still be reported without loading the harness. Directory
changes and skipped locations are visible. Terminal control characters in paths
are escaped in the plain report.

Limits: 16 roots, 4,096 directory entries, 512 directories, depth 8 below a root,
256 session files, and a 128 MiB aggregate session-read reservation. File sizes
are charged even for failed inspections; each read may use one additional byte
to detect growth beyond its reservation. Caps report truncated coverage. Order is
by path, not recency; a bounded subset is not proof of what was busy before reboot.

One locked policy snapshot is used for the batch. Native-ID disables apply to
copies/moves, and every retained attempt remains a blocker. Discovery may create
Reignite's private directory/lock, but never `state.json`, enrollment records,
a lifecycle ledger, migration or edits to native files. Corrupt policy fails the
whole operation; it is not replaced with default enablement.

For scripts, `reignite discover --json` returns the existing JSON envelope.
The request equivalent is `{"op":"discover_pi","sessions_roots":["/absolute/root"]}`;
omitted/empty roots use the default selection. Other CLI commands keep their
existing JSON output. This batch does not join optional usage logs; selected-file
assessment still supports them separately without rescanning a shared log for
every session.

## What this does not prove

Discovery can be invoked after a restart, but does not install a boot hook,
record a new boot history, or prove that a reboot interrupted work. There is no
unattended continuation, reconstructed human callback, exclusive execution lease,
original permission-profile guarantee or remote-child death inference.

Rust CLI tests cover ordinary/custom locations, nested files and aliases,
read-only policy/attempt retention including schema 1, corruption, unsafe sources,
limits and readable terminal-safe output. The real offline SDK smoke creates a
fixture through Pi's native writer in its ordinary storage, finds it without
enrollment, and checks unchanged bytes. That is native storage compatibility,
not live Kitu observation, model work or full-reboot acceptance.
