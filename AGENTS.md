# Reignite

Build a small standalone recovery component for interrupted coding-agent sessions.
Rust owns durable state and recovery decisions; harness adapters observe lifecycle
and deliver continuation. tmux-assistant-resurrect remains the session launcher.

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
- Recovery is opt-in and limited to an explicit restore context after a host boot
  change. Ordinary manual session reopening must not start unattended work.
- Never imply exactly-once tool execution. Unknown delivery or tool outcomes must
  remain visible, with no automatic replay of side effects.
- Keep adapters small and report unsupported lifecycle states honestly.
- Test observable behavior at the real CLI and adapter boundaries; avoid a new
  framework, dashboard, daemon, or unrelated session-management features.
