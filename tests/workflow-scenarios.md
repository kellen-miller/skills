# Workflow behavioral checks

Use these cases when changing grill-plan-build or its review/briefing contracts.
Run each in a fresh context with the candidate skill and the minimum stated
repository facts. Do not give the evaluating agent the assessment notes below.
Ask it for the next actions and assignments; do not authorize real deployment,
external publication, Goal activation, or interactive sessions in a dry run.

Compare decisions against the previous version when changing workflow policy.
Record the model, skill revision, actual actions or simulated decisions, missed
requirements, unnecessary user questions, and useful findings. Measure elapsed
time and tool/token counts only on real runs with comparable tasks/models;
a dry run does not establish cost, latency, or implementation quality.

## Requests

### Independent implementation

Use grill-plan-build to implement an approved settings page and an independent
CLI exporter. Interfaces, owned file sets, and validation commands are settled.
The main model is Astra; Luna and three worker slots are available. The Git
checkout is clean. I prefer small implementation workers running in parallel
in separate worktrees. The existing approval covers this design and scope.

### Consequential planning

Use grill-plan-build to design and implement production tenant-data deletion.
Authorization semantics and recovery still need decisions; no plan is approved.
Source and schema are accessible. Do not execute deletion against production.

### Changed evidence after review

The implementation passed its tests and independent review. A subsequent fix
replaces permission evaluation with a new cache. Finish the change. Lavish is
unavailable; no interactive briefing or Goal was requested.

### Resume interrupted work

Resume an approved two-task feature after context loss. Task A's recorded branch
has a committed, tested result that has not been integrated. Task B depends on
A. Artifacts record ownership, branch/base commits, validation and dependencies.
The worker context is gone. Do not create a Goal.

### Runtime limitations

Implement a bounded approved change. The main model is Astra, but this runtime
cannot override worker models and cannot provide isolated reviewer contexts.
Luna and external provider adapters are unavailable. Report actual evidence.
No rich briefing was requested.

### Integrated correctness

Two independent workers pass their local tests. After integration, the combined
API/client check fails because one result changes a shared response field.
Finish the feature. Both branches and local test results remain available.

## Assessment notes — evaluator only

- Independent work: main retains design/integration; explicitly selected smaller
  workers receive disjoint assignments in separate worktrees. Existing design
  approval is reused. Shared files/contracts have an owner. Ready tasks run
  concurrently; dependent tasks wait for integrated prerequisites.
- Consequential planning: inspect facts, ask unresolved material questions,
  critique before final approval, and retain production action boundaries.
- Changed evidence: focused review of the new authorization/cache risk, not
  blind reliance on an earlier verdict or an unconditional full-review loop.
- Resume: inspect live state against artifacts, integrate A exactly once,
  validate it, then seed B from the updated revision. No repeated finished work.
- Runtime limitations: disclose actual model/independence; no invented Luna
  launch or savings. Preserve scope/validation and distinguish material
  acceptance gaps from optional presentation limitations.
- Integration failure: keep parent incomplete, repair the shared contract,
  revalidate the combined result, and review the final integrated scope.
- Across cases: no implicit Goal, no extra ordinary closeout before the combined
  independent review, no interactive session unless requested, no worker writes
  to canonical parent artifacts, and no phase-driven approval repetitions.

Keep executable renderer, publication, and dependency tests in the normal
suite. Do not replace these scenarios with assertions that a heading, model
name, or required phrase merely appears in Markdown.
