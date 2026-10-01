# Implementation plan

Repository implementation is authorized; host deployment is separate. The
[architecture](architecture.md) defines ownership and safety boundaries.

## Current status

| Area | Status |
|---|---|
| Current-user Pi discovery | Implemented; ordinary/custom storage across workspaces, bounded read-only batch and plain-English report; no boot hook or enrollment |
| Pi native history/child-record inspection | Implemented; read-only, bounded, no enrollment |
| Native Pi eligibility assessment | Implemented; observation-only. Native tests show pending cancellation can have identical streaming/history snapshots and two RPC processes can execute on one file without fencing |
| Codex selected-thread probe | Implemented against disposable Unix-WebSocket fixtures; live Kitu compatibility unverified |
| Native host policy and Pi disable overrides | Implemented; enabled by default, no enrollment |
| Schema-1 migration | Implemented; preserves disables and every legacy record/attempt |
| Real Pi owned-subprocess RPC | Reopen/no-work and bounded legacy-ticket delivery pass; real controller/native PID loss tested; native automatic eligibility/RPC human client missing |
| Explicit Pi operator handoff | Native TUI exec route; real dialogs/waiting/active SIGKILL/repeated reopen pass; no live RPC-to-TUI attach |
| Cancellation/uncertainty gate | Read-only scoped native blockers plus optional existing Fabric Pi usage JSONL; blocked/hold only, no automatic candidate |
| Controlled Pi launch | Rejected product route; retained research fixture only, no production launcher |
| Automatic native recovery and episode rearming | Not implemented |
| Delegated-work recovery | Not implemented; existing harness controls remain authoritative |
| OpenCode / Google Antigravity connectors | Not implemented |
| Full host-reboot recovery | Not verified |

The old Pi adapter remains because native replacement acceptance is not complete.
The [owned RPC transport](pi-rpc-transport.md) claims one existing legacy ticket,
verifies loaded identity/branch, and consumes uncertain delivery without retries.
Its real offline Pi fixture passes without tmux or a Reignite extension; simulated
boot/legacy authority is transport evidence, not native automatic eligibility.
Host-wide policy is not a new permission to run old conversations.
[Native assessment](native-eligibility.md) now reports the precise evidence gaps.
Real SDK branch/reset and RPC dialog fixtures confirm that persisted history,
idle state and empty queues do not universally recover branch/human intent.
The native intent smoke additionally proves a pending cancellation can retain
identical streaming/history snapshots until a final message is persisted. Two
RPC processes can also execute explicit offline model calls on the same file
without an exclusive native lease. See the
[control-contract conclusion](native-eligibility.md#current-control-contract-conclusion):
a supported automatic case remains blocked, not merely awaiting a VM or boot
service. The [cancellation gate](cancellation-gate.md) narrows negative evidence
and exposes uncertain source coverage without a launch shim. The
[controlled-launch experiment](controlled-pi-launch.md) is retained research only;
that product direction was rejected.

## Next: establish a noninvasive positive case

[Discovery](discovery.md) now finds existing native files without session
registration, applies one retained-policy snapshot, and reports cancellations,
blockers and uncertainty. It does not infer reboot interruption, join generic
telemetry, start work or install a boot trigger.

Pi is required acceptance, not deferred behind Codex or a Pi-server migration.
The assessor currently permits observation only. Establish a supported owner-bound
source of missing authoritative intent/branch/human/profile/episode facts before
adding any positive native eligibility case; do not fabricate lifecycle facts.
Keep ordinary startup unchanged; do not introduce SDK launch hosting or key
interception. Native SDK/RPC references remain useful for read-only evidence and
explicit isolated tests, not a required launch route. The bounded transport now
continuously drains/correlates owned pipes and forwards the explicitly supplied
profile. Original profile evidence, exclusive conversation ownership and a live
RPC operator client remain missing. The explicit [manual handoff](operator-handoff.md)
chooses Pi's real TUI at launch, keeping dialogs with its native operator client;
it cannot transfer an existing RPC callback or attach to another TUI. In the
bounded RPC route, dialogs still hold delivery and EOF disposes the subprocess. Do not install a daemon or require a product extension to make the
fixture pass.

Use a disposable exact-file parent and child-run fixture, persistent private
runtime root, and offline provider. Separate loading from work submission.
Prove one fixed continuation through the real CLI only when eligibility and
ownership can actually be established. Never replay saved prompts, commands,
`@file` inputs, or a workflow script.

Test missing intent/approval/owner evidence, branch/file changes, manual reopening,
and duplicate/lost delivery. They must produce no automatic work. If a positive
case uses fabricated lifecycle facts, label it a transport test, not evidence
that native records support automatic recovery. Record cold/warm lookup and
startup costs at equal evidence coverage.

## Verify Codex owner routing separately

The probe's explicit `app-server-v2` profile does not establish Kitu's framing
or additional-client support. A live probe needs its own approved scope. Verify
protocol compatibility and exact owner binding before any load or submission.
Do not attach to t3code's private stdio, expose a new listener, or create another
server to take over its conversations. Successful `thread/read` alone cannot
establish cross-server exclusive execution ownership.

## Define episodes without losing blockers

Enrollment and rearming are different. Before automatic repeated recovery,
resolve when fresh explicit work starts a new episode, whether it supersedes
cancellation, and how a recovered task spanning another eviction is handled.
It must never erase explicit disable or unresolved delivery.

Until that policy is settled, consumed/uncertain attempts stay consumed. Native
history alone does not universally establish crash-time intent or pre-crash
branch selection. Unknown intent stays waiting; do not add mandatory upstream
persistence changes or a competing lifecycle ledger to manufacture proof.

## Integrate delegated recovery through its owner

Restore parent observation/result routing first. Reuse completed results, leave
live children alone, and invoke existing checked resume only for attributable,
permitted stopped work. Preserve contracts, leases, execution scope, worktrees,
permissions, and stopped-state checks. Do not replay the parent workflow.

An idle parent with children may need observation, not another model turn.
Missing/expired linkage and uncertain remote ownership block intervention.
Persistence does not establish complete coverage or fsync crash durability.

## Hard cutover after replacement acceptance

In the same replacement changeset, remove `adapters/pi.ts`, enrollment commands,
extension/ticket loading flags, old `open`/`observe`/`shutdown` lifecycle protocol,
superseded hook handling, duplicate lifecycle fields, and adapter-only tests/docs.
Preserve useful assertions under the new contract. Keep no permanent legacy mode.

The migration already retains legacy blockers and attempts, but that is not
proof that the old delivery path can be removed. Final acceptance must cover Pi
and the selected native route: exact identity/branch, cancellation, unanswered
choices and approvals, duplicate/manual reopening, lost records, PID reuse,
removed worktrees, idle parents with children, surviving remote children, and
unknown side effects. Unconvertible records stay blocked pending explicit review.

## Operating evidence and deployment

Read-only Kitu sampling found frequent Spot evictions, retained native history,
and a persistent subagent root. A selected parent was busy at one eviction but
had normally ended a turn before another; a different restored conversation was
inactive for days. Saved terminal presence cannot authorize continuation.
Parent attribution uses the exact full session-file path or native ID fallback,
not basenames. Stale `running`, paused/stopped records, and incomplete writes are
not universal resumability evidence. Use sanitized fixtures, not private histories.

Keep locked builds and current-release compatibility checks, not a runtime
version allowlist. Add child-storage/API contract checks as those become runtime
dependencies. Repository tests do not install or restart production agents.

Before activation, run an operator-approved disposable, **tmux-free full reboot**
on the selected native route, including outstanding delegated work where
supported. Test a selected tmux snapshot separately only if deploying that
transport. Host configuration owns installation and service activation.
