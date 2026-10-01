# Reignite

Reignite is being built to help you pick up coding-agent work after a Linux
machine restarts—for example, when a Spot VM is evicted.

Today, you can inspect saved Pi sessions, check their recovery settings, and
reopen them in Pi to continue manually. **Automatic recovery is not implemented
yet.** This is an experimental project, not a service you can leave to resume
agents unattended.

## What you can do now

- Find saved Pi sessions across workspaces without registering each one. Get a
  plain-English report of recorded cancellations, blockers, and uncertainty.
- Read a Pi session without launching the agent. See its saved history metadata
  and any recorded child-agent runs, without printing prompts or tool arguments.
- Check what would prevent safe recovery, including recorded cancellation,
  disabled recovery, earlier attempts, and missing information. The read-only
  [cancellation gate](docs/cancellation-gate.md) can also check an existing Pi
  usage log; uncertainty stays on hold.
- Open the selected session in Pi's normal terminal interface. You decide what
  happens next; Reignite sends no prompt and answers no questions.
- Turn recovery off for a session without stopping its current work. The setting
  still applies if you move or copy the session file.

Codex support is limited to a read-only status probe tested against simulated
servers. OpenCode and Google Antigravity support have not been implemented.
An [older opt-in prototype](docs/legacy-prototype.md) remains for testing; it is
not the planned standalone recovery system.

## Try it

These build commands need Linux and Nix. Discovery finds your normal Pi storage
without a session path. For the individual-file commands, replace the example
path with an existing Pi session file. It must belong to you, have no symlinks in
its path, and not be writable by other users.

```sh
nix build

# Find saved sessions and explain blockers in plain English. Starts no work.
result/bin/reignite discover

# Inspect a saved session without starting work.
result/bin/reignite inspect --session /full/path/to/session.jsonl

# Check recovery settings and explain what is still unknown.
result/bin/reignite assess-pi --session /full/path/to/session.jsonl

# Disable recovery for this session. Normal Pi use is unaffected.
result/bin/reignite disable --session /full/path/to/session.jsonl
```

Discovery prints a plain-English report; use `discover --json` for scripts.
The other commands above return JSON. Building Reignite does not install a
service or change your agent configuration. See the [CLI reference](docs/cli-reference.md)
for manual reopening and the other commands.

## What recovery must respect

A restart is not an answer to a question or permission to run a command. Cancelled
work must stay cancelled, and disabled recovery must stay disabled. If a tool may
already have finished, restarting the machine is not a reason to run it again.

The goal is to use the agents' existing interfaces and session files, without a
required Reignite extension or a modified agent. tmux is optional. Reignite must
not restart child agents that are still running elsewhere.

## Tests and remaining work

Tests exercise real Pi sessions, unanswered dialogs, manual reopening, and abrupt
process loss. GitHub checks pass on x86_64 and ARM64 Linux, with a separate check
against current Pi and tmux releases.

**A full host reboot has not been tested.** Automatic continuation, recovery of
child-agent work, and handling repeated restarts still need work. The
[implementation plan](docs/implementation-plan.md) tracks those gaps.

Run the basic checks with:

```sh
nix develop --command bash scripts/check.sh
```

The [testing guide](docs/development.md) covers real Pi tests in disposable
sessions, without provider credentials or changes to live services.

## Documentation

- [Finding saved work](docs/discovery.md)
- [Commands](docs/cli-reference.md)
- [Recovery settings and saved state](docs/policy.md)
- [Manual Pi reopening](docs/operator-handoff.md)
- [Design and safety boundaries](docs/architecture.md)

## License

[MIT](LICENSE).
