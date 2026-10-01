# Reignite

Build a small standalone recovery component for interrupted coding-agent sessions.
Pi is a required target; Codex-only acceptance does not complete the replacement.
Target host-wide, storage-first recovery without a required Reignite extension
or per-session enrollment. Rust owns host policy, disable overrides, and recovery
attempts; harness-owned records supply authoritative session and execution facts.
Prefer existing harness APIs and child-resume controls. tmux is optional terminal
presentation, not a core dependency. Loading a session and starting model work
are separate decisions. Replace superseded runtime paths rather than keeping
parallel implementations.

- This is an experimental Linux-first project, not production-ready recovery.
  Publishing, pushing, or creating a remote requires explicit operator approval
  and a license decision. Keep `publish = false` until a crates.io release is
  separately approved.
- Do not install into live agent configuration, restart tmux, or reboot the host
  as part of repository work. Deployment verification requires separate approval.
- Keep changes within this repository. Existing infrastructure and installed Pi
  files are read-only references.
- Use the Nix flake development shell. `nix develop --command bash scripts/check.sh`
  runs local checks; `--integration` also requires the installed Pi/plugin fixtures.
  Do not describe the default checks as real Pi or reboot verification.
- Host policy controls enablement; per-session enrollment is not the intended
  default. Explicit disable, cancellation, and approval waits remain blockers.
  Recovery requires an authorized restore after a host boot change. Ordinary
  manual session reopening must not start unattended work.
- Never imply exactly-once tool execution. Unknown delivery or tool outcomes must
  remain visible, with no automatic replay of side effects.
- Keep harness-specific readers and delivery code small. Report missing evidence
  honestly; history is not authorization. Use existing harness storage and launch
  interfaces without requiring forks, extra extensions, or a competing lifecycle
  ledger. Missing safety evidence blocks automatic continuation.
- Test observable behavior at the real CLI and adapter boundaries; avoid a new
  framework, dashboard, daemon, or unrelated session-management features.
