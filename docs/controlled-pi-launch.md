# Controlled Pi launch: rejected product route, retained research

This experiment hosted Pi's public SDK and native terminal interface. It needed
no fork or Reignite extension, but it still changed startup and intercepted input.
The operator rejected that invasive product direction. It is not the implementation
plan, automatic recovery, or a route for taking over existing sessions. The
[current cancellation gate](cancellation-gate.md) reads existing records without
changing how users launch or cancel Pi.

## What works in the real experiment

`scripts/pi-owned-launch-smoke.py` runs `tests/pi-owned-host.mjs` in private
terminals with a disposable offline provider. It uses exported
`createAgentSessionRuntime`, `createAgentSessionServices`,
`createAgentSessionFromServices`, `SessionManager` and `InteractiveMode`.
`InteractiveModeOptions.terminal` accepts the ordinary public `Terminal`
interface. A delegating terminal forwards rendering to `ProcessTerminal` but
can perform a synchronous safety operation before forwarding input to Pi.
No private Pi fields or methods are read or patched.

The actual order tested is:

1. An explicit keyboard prompt starts real offline provider work.
2. The fixture sends Escape to Pi's normal terminal interface.
3. Before forwarding that input, the host runs Reignite's existing native-ID
   disable command. Its locked policy store writes and syncs the disable.
4. Only then does Pi receive Escape and abort its native run.
5. The provider is deliberately slow to finish cancellation. Its final aborted
   message is not yet in history when the test kills that exact owned PID.
6. The durable disable survives. Assessment is blocked even though the saved
   history still looks like unfinished work.
7. A fresh SDK/TUI backend loads the same file without starting model work. A
   test extension's native selector stays unanswered until explicit input.

This closes the **tested terminal-input cancellation gap** without a required
observer extension. The fixture conservatively disables on every input during
work; it does not interpret keybindings, scrape terminal output, or guess intent
from messages. It never clears that disable automatically. That is a proof of
ordering, not the final per-work recovery policy.

The native core also exposes an active `AbortSignal` to `agent.subscribe`
listeners. The fixture observes it to verify native cancellation actually
happened. An abort listener alone is **not** a durable pre-cancellation gate:
the signal has already changed when the listener runs.

## What the lock proves—and what it does not

The test acquires a Linux file lock before starting the managed backend and
passes its descriptor to that backend. The client closes its descriptor. A
second participating launcher cannot obtain the lock while the backend lives;
killing the backend releases it. The terminal client does not own the lock's
lifetime.

This is useful for preventing duplicate **managed** starts. It cannot stop a
plain Pi process or SDK application that ignores the lock. Do not label it
exclusive native conversation ownership or use it to authorize automatic work.
The earlier [native intent test](native-eligibility.md) demonstrates that bypass.
Real detach/reattach through a terminal broker is also not tested here.

## Why this is not the product route

An unchanged native renderer does not mean unchanged normal startup. This fixture
requires a hosting wrapper and intercepts input before Pi receives it. That does
not meet the accepted noninvasive design.

The ownership result is scoped to cooperating launches. Under the trusted-same-UID
policy, a hypothetical managed namespace could exclude unmanaged clients from
its stated guarantee; this experiment still does not establish a native lease or
permission to recover ordinary sessions. No account/container/storage boundary
is proposed or installed.

Other unproved areas include non-terminal cancellation, extension-triggered
operations, complete CLI-profile parity, per-task authorization and rearming,
repeated interruptions, lost native callbacks, and delegated-owner survival.
Preserve the fixture as evidence of those limits, not instructions to implement
a managed launch path. No VM, service, global launcher, account, installed Pi file,
Kitu agent or live configuration was changed by this research.
