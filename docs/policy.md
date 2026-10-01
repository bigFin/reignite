# Policy, state, and migration

Policy is implemented; automatic standalone recovery is not. A policy decision
is not execution ownership, restore authorization, or an answer to a question.
See the [architecture](architecture.md).

## Host-wide defaults and session overrides

```sh
reignite policy show
reignite policy show --session /absolute/canonical/session.jsonl
reignite policy disable
reignite policy enable
reignite disable --session /absolute/canonical/session.jsonl
reignite policy clear-disable --session /absolute/canonical/session.jsonl
```

The default is enabled for supported sessions belonging to one trusted Unix user.
There is no enrollment step for native policy. This does not install a boot hook,
start a service, or make native automatic delivery available. Per-session overrides
currently support native Pi files; Codex/OpenCode/Antigravity session controls
are not implemented.

A disable is keyed by harness and native session ID, not filename or inode. It
follows moved/copied files with the same ID. File/workspace paths are provenance,
not a second authoritative identity. A different ID cannot clear the old disable.
Host enable does not erase session overrides. Ordinary reopen/observation does
not erase them either. Disabling recovery does not abort ordinary harness work.

Session controls read at most a 1 MiB complete version-3 header from a canonical,
user-owned regular file that is not group/other writable. They do not scan or
copy history. A missing workspace, incomplete tail, or large history does not
prevent opting out; those conditions may still prevent inspection or recovery.
Symlinks, unknown headers, or incomplete/oversized headers are rejected.

`policy show` reports policy separately from retained legacy blockers/attempts.
Its `eligible` is always false. New native `recover --dry-run` calls report policy
blockers or missing implementation/evidence; they never enroll or authorize a
session. Only the transitional registered-session route can issue a restore ticket.

## Enablement is not rearming

Host enable/disable and `clear-disable` do not reset attempts, change retained
legacy records, clear cancellation/shutdown facts, or submit work. An authorized,
claimed, or accepted attempt remains exactly as recorded. Host toggles gate an
existing ticket; they do not revoke, renew, or replace it. Unclaimed legacy
tickets retain their original 120-second expiry and identity checks.

Removing a native override does not arm a disabled legacy record. The old explicit
`/recovery enable` command still performs its documented manual attempt reset and
clears the selected native-session override, only while idle/stopped and with
host policy enabled. This is a transitional exception, not the behavior of the
new policy commands. Do not use it to conceal uncertain delivery.

Future repeated-eviction policy must define new work episodes without blindly
retrying the previous one. Fresh user work, cancellation, explicit disable, and
unknown delivery remain separate decisions; policy defaults do not settle them.

## One state store

`--state-dir` or `REIGNITE_STATE_DIR` selects an absolute canonical private state
directory. Default: `$XDG_STATE_HOME/reignite`, or `$HOME/.local/state/reignite`.
The schema-2 `state.json` holds host policy, disable overrides, and retained
legacy records/attempts. No prompts, transcripts, commands, or child contracts
are copied into it.

One `flock` serializes reads/mutations. Writes use a private exclusive-create
file, file fsync, rename, and directory fsync. State and lock files must be
user-owned, non-symlink, and private; parent directories must be trusted. Only
local Linux filesystems are supported. Serialized state is capped at 16 MiB;
there is no garbage collection.

`status`, `policy show`, and recovery dry-runs may create the private directory
and lock, but do not rewrite `state.json`. Pure `inspect` and `probe-codex` create
no recovery state. State is trusted operator data: any process with the same
Unix identity, including an agent tool, can change it. This is not a sandbox.

## Bounded schema-1 migration

Reading schema 1 derives policy in memory. A successful mutation commits schema 2
atomically; failed operations and read-only queries leave the old file unchanged.
Migration retains every legacy record and all attempt IDs, boots, expiry times,
and statuses. Disabled or stopped records become native disable overrides.

Old disabled records can reflect explicit opt-out, cancellation, manual reopening,
or missing enrollment. Migration cannot reconstruct that intent, so it preserves
the blocker conservatively rather than automatically enabling those sessions.
Expired, claimed, or accepted attempts are never silently cleared. An acceptance
receipt arriving after disable can still be recorded without re-enabling recovery.

Unknown/corrupt schemas, invalid identities, unsafe state permissions, or a
migration exceeding the store cap fail without replacing the original. Repair
or resolve them explicitly; do not fabricate clean state. Older schema-1-only
binaries cannot read schema 2. Deployment/rollback must account for that boundary.
