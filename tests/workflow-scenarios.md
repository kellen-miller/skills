# Workflow behavioral checks

Use these cases when changing grill-plan-build or its dispatch/review/PR explanation contracts.
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
replaces permission evaluation with a new cache. Finish the change. No standalone briefing or Goal was requested; explain the final lifecycle in
the authorized PR with observed evidence.

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

### Task-local execution scope

The approved parent plan has UI, API, and database milestones. Delegate only
the UI task to a Luna worker using implement-execplan. The repository's
PLANS.md is available. Prepare its launch packet and task-local artifacts.
During the task it discovers a mismatch in an approved assumption.

### Repair a previously integrated task

Worker A branched at S and returned commit A1, which main cherry-picked into
integration as I1. Other tasks are now integrated through I3. Retain A's useful
context to fix a bug against the combined code, preserving its previous work.
State its repair workspace, starting commit, return evidence and transfer range.

### Portable CLI loop

Use grill-plan-build to implement an approved three-ticket graph. Tickets A and
B own disjoint paths; C depends on A. The worktree is clean. Codex and Claude
CLIs are available, and I want Claude for A, Codex for B and C. CLI permissions
and authentication are already configured. Run a deterministic local loop;
do not publish tracker issues. Open a visual PR after validation and review.

### Failed integration checks

Ticket A has merged but combined checks failed. Ticket B depends on A and has
not started. Diagnose the failure, preserve all work, and resume using the
recorded graph. The failure was an environment prerequisite, now restored;
no code or approved scope changed. Do not implement or merge A again.

### ExecPlan, graph, and Goal together

Use the full grill-plan-build workflow to implement an agreed feature and
create a Goal for execution after I approve the plan. The decision ledger is
complete, but no ExecPlan exists. Both planning skills, Goalcraft, and native
Goals are available. The repository has no PLANS.md. Two independent slices
and a dependent integration slice are expected. Describe actions before and
after approval, including when implementation is complete.

### Approved design without Goal authorization

Use grill-plan-build to execute this existing approved ExecPlan and task graph.
My earlier approval covered the design only. No Goal request or activation
approval has been given. Describe the next step and which planning work is reused.

### Planning-only handoff

Create and improve the ExecPlan and task graph for this resolved design.
Planning only: do not implement or activate a Goal. Native Goals are available.
Explain what you produce and where you stop.

### Resume with an unfinished Goal

Resume this approved work item with an unfinished Goal already pointing at its
ExecPlan and graph. One task is integrated and verified; another is ready.
Describe how you resume. Also describe what changes if the active Goal instead
belongs to an unrelated work item and replacement has not been authorized.

### Goal activation unavailable

The plan, graph, implementation, and Goal activation are approved. Goalcraft is
available, but this runtime has no native Goal tool. Implementation is authorized
even if persistent Goal execution is unavailable. Describe your fallback and
the evidence required before reporting completion.

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
- Task scope: the worker's executable plan contains only its assignment;
  parent milestones remain read-only context. Return discoveries and decision
  changes so main can reconcile canonical intent and approval.
- Repair: fresh worker branch/worktree starts at I3 containing the integrated
  result; return and apply only new commits after that exact starting commit.
  Prior worker state is preserved, and A1 is not transferred a second time.
- ExecPlan lifecycle: invoke execplan-create for a new plan and execplan-improve
  once before deriving/finalizing its graph. Follow the PLANS.md contract; do not
  replace either skill with an informal plan. No repeated passes for reassurance.
- Goal lifecycle: after plan approval and explicit activation authorization,
  invoke Goalcraft to validate and activate a compact objective referencing the
  work item. Main remains orchestrator; dispatcher remains the sole scheduler.
  A finished graph does not complete the Goal before validation/review/handoff.
- Approval boundary: reuse approved designs and plans. If only Goal permission
  is missing, request it without restarting planning. Do not infer authorization
  from a skill being selected implicitly, silence, or unrelated approval.
- Planning-only: create/improve plan and graph, without implementation or Goal
  activation. An explicit no-Goal request also permits authorized implementation
  to proceed without repeatedly asking for activation.
- Goal resume: reuse the matching unfinished Goal and verified task state;
  do not rerun completed planning or replace an unrelated Goal without consent.
- Missing Goal runtime: retain the validated objective, disclose non-activation,
  and continue authorized implementation without claiming a native Goal exists.
- Across cases: no extra ordinary closeout before the combined independent
  review, no interactive session unless requested, no worker writes to canonical
  parent artifacts, and no phase-driven approval repetitions.

- Portable loop: select dispatch-tickets, preview the approved immutable graph,
  use explicit provider commands per ticket, and let the script own scheduling,
  local commits, serial merges, and checks. Main retains review and publication.
  Evidence in the PR distinguishes fixture checks from live model execution.
- Integration recovery: inspect retained state and logs, restore the prerequisite,
  explicitly retry A's checks, and unblock B only after combined checks pass.
  Do not create another implementation attempt for the already merged result.


Keep executable dispatcher, publication, and dependency tests in the normal
suite. Do not replace these scenarios with assertions that a heading, model
name, or required phrase merely appears in Markdown.
