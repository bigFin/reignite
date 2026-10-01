# Reignite

Standalone recovery for coding-agent work interrupted by Linux host restarts.
Pi is a required target. Recovery policy belongs at the host level; tmux is
optional presentation, not the authority for starting work.

**Experimental; automatic standalone recovery is not implemented yet.** Current
commands inspect/assess Pi storage, probe a selected Codex connection, and manage
durable recovery policy. They do not automatically resume agents or answer questions.
An older delivery prototype and experimental owned-Pi RPC ticket transport remain
during the transition; neither establishes native automatic recovery eligibility.

## Design

- Use supported harness APIs and native storage, without forks or a required
  Reignite extension.
- Default to host-wide policy, with durable per-session disable overrides.
- Keep process restart, conversation loading, and starting work separate.
- Leave unanswered questions, choices, and approvals waiting after a reboot.
- Keep child ownership and checked resume with the harness, not a new workflow
  engine. Unknown tool outcomes must never trigger blind replay.

See the [architecture](docs/architecture.md) and [remaining work](docs/implementation-plan.md).

## Build and try

```sh
nix build
result/bin/reignite inspect --session /absolute/canonical/session.jsonl
result/bin/reignite assess-pi --session /absolute/canonical/session.jsonl
result/bin/reignite policy show --session /absolute/canonical/session.jsonl
result/bin/reignite disable --session /absolute/canonical/session.jsonl
```

Inspection needs no extension or enrollment and creates no recovery state.
Disabling recovery also needs no enrollment; it saves an override without
stopping ordinary harness work. Host policy defaults to enabled, but that is
not an installed boot service or permission to resume an ambiguous session.
[Native assessment](docs/native-eligibility.md) reports policy, retained blockers,
raw history signals and missing authority; it permits observation only.

A read-only Codex Unix-WebSocket probe is also available. Its protocol fixtures
pass; compatibility with Kitu's managed socket remains unverified. Pi's real RPC
reopen and offline one-use ticket-delivery tests pass without tmux or a Reignite
extension. An explicit [operator handoff](docs/operator-handoff.md) uses Pi's real terminal
client; local dialog/active-work PID-kill tests pass. Live RPC human routing and
native automatic eligibility remain unfinished. OpenCode
and Google Antigravity connectors are not implemented. No route has passed a full host-reboot test.

## Development

```sh
nix develop --command bash scripts/check.sh
```

Build dependencies are locked. CI checks both Linux architectures and separately
checks current Pi/tmux compatibility. Repository work installs nothing into live
agents and does not change host services.

## Documentation

- [CLI reference](docs/cli-reference.md)
- [Policy, state, and migration](docs/policy.md)
- [Development and verification](docs/development.md)
- [Transitional Pi/tmux prototype](docs/legacy-prototype.md)

## License

[MIT](LICENSE).
