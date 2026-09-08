---
name: grill-plan-build
description: >-
  Use when the user requests grill-plan-build, their full grill/plan/build
  workflow, or a complex feature, refactor, migration, or architecture change
  requiring resolved design decisions and a durable implementation plan.
  Do not use for tiny edits or when the user wants only an answer or review.
---

# Grill Plan Build

Resolve intent, write an executable plan, implement through smaller agents,
validate the integrated result, obtain independent review, and explain it.
The user-selected main model owns grilling, planning, coordination, integration,
and acceptance. Keep those decisions with the capable main model; delegate
bounded coding work to smaller models in separate worktrees.

User instructions and existing authorization take precedence over skill
preferences. Preserve approved decisions; ask only about unresolved judgments
that could materially change the result. Do not restart grilling or approval
merely because a phase or agent changed.

## 1. Establish Scope And Workspace

Inspect the relevant source, repository instructions, Git state, and applicable
ADRs before proposing a design. Distinguish unresolved planning from a large
implementation whose design is already settled.

If major unresolved decisions or investigations make planning span sessions,
propose a destination and ask the user to invoke `$wayfinder`; do not create
its map or start implementation. For an active map, return to its open tickets
or in-scope fog. For a completed map, reuse its destination and linked decisions
without re-grilling them. Keep the map URL in the work item when applicable.

Use `$using-git-worktrees` to select an isolated integration worktree before
writing artifacts or source. Reuse the current managed worktree if already
isolated; preserve the primary checkout and unrelated changes. Honor an
explicit in-place instruction. Inspect and record branch, base ref, starting
commit, and upstream rather than assuming tracking is safe. Follow repository
branch and publication rules; workflow invocation does not authorize deployment
or unrelated external writes.

If Git or delegation is unavailable, retain the same decision, validation,
and acceptance responsibilities locally and disclose the actual limitation.
Do not simulate isolation or claim parallel execution that did not occur.

## 2. Grill And Plan In The Main Context

The main agent invokes `$grill-me`, inspects discoverable facts itself, and
asks the user about unresolved intent, tradeoffs, failure behavior, and scope.
Reuse an existing approved spec or decision ledger. Do not relay each question
through a separate grill agent or create a new planning agent by default.

Apply supporting lenses only when they change the work:

- `$domain-modeling` for unclear terminology, lifecycle, ownership, or ADRs;
  `$grill-with-docs` may combine this with grilling when available.
- `$codebase-design` for module boundaries and public interfaces.
- `$frontend-design` for UI shape, interaction states, and browser evidence.

Honor existing ADR conventions. Promote only durable, non-obvious architectural
tradeoffs whose rationale matters beyond this work; let `$domain-modeling`
own authoring and format. Preserve accepted rationale and supersede it with a
new decision when intent changes. Keep ordinary delivery decisions in the plan.

In Git repositories, verify `.agent/work/` is ignored in each worktree before
writing its work items. Use existing rules or a Git-local exclude when needed;
keep plans, review packets, and briefings out of commits. The main agent writes
the canonical work item in the integration worktree:

```text
.agent/work/<slug>/
  decision.md
  execplan.md
  meta.json
```

- `decision.md`: objective, confirmed user decisions, assumptions, non-goals,
  material risks, and approval/provenance. Distinguish user decisions from
  agent recommendations. Link relevant ADRs or completed Wayfinder decisions.
- `execplan.md`: intended behavior, source-grounded changes, ordered milestones,
  task ownership/dependencies, observable acceptance checks, and rollout or
  recovery when relevant. Keep progress, discoveries, and decision changes
  current. Follow the repository's `.agent/PLANS.md` when present.
- `meta.json`: lifecycle (`stage`, `state`) and explicit artifact paths. Use
  `stage="plan"` while planning, then `stage="implementation"` while executing;
  use `state="active"`, `"blocked"`, or `"completed"` according to evidence.

Check paths, interfaces, dependencies, feasibility, and validation while writing
the plan. `$execplan-create` can assist with a complex repository plan format;
`$execplan-improve` is for a concrete uncertainty, stale plan, or difficult
boundary, not a mandatory pass count. Do not invoke `$grillcraft` as a second
orchestrator or duplicate the plan in a Goalcraft objective.

For consequential architecture, security/authz, billing, data loss, destructive
migration, difficult recovery, or unresolved design risk, invoke
`$adversarial-review` on the plan **before final plan approval**. Include the
applicable domain/frontend lens in that review, without adding a second event.
For ordinary bounded work, source-grounded planning is sufficient. Record why
planning critique was needed or skipped; risk determines useful evidence and
review depth, not a fixed ceremony for every task.

Verify findings and revise the plan. Present the resulting scope, decisions,
implementation split, risks, and acceptance checks for explicit approval.
Existing approval suffices if it covers this scope and design. Record the
approved revision and material decisions; progress updates and editorial fixes
do not require reapproval. Obtain approval for material changes to agreed
intent, not routine implementation choices within it.

## 3. Dispatch Smaller Implementation Agents

Prefer parallel implementation whenever the approved work has independent,
bounded tasks with settled interfaces. Use one worker for tightly coupled work;
do not invent subdivisions merely to fill slots. The main agent owns the split,
shared contracts, sequencing, and integration instead of doing every coding
slice itself.

### Model Selection

- Keep the user-selected capable main model (for example Astra) as orchestrator.
- Default bounded implementation tasks to `gpt-5.6-luna` when the runtime exposes
  it, unless the user selected another implementation model. Otherwise select
  an available smaller coding model and disclose the concrete fallback.
- Explicitly set worker `model` and supported `reasoning_effort` in the launch
  API. With the current collaboration tool use `fork_turns: "none"` so the
  model override is effective. Start with reasoning appropriate to the slice;
  increase it when the actual difficulty warrants it.
- If overrides are unavailable, disclose the inherited model and limitation.
  Record requested/observed model and effort with the task; do not present a
  requested value as confirmed runtime evidence or claim smaller-model savings
  when the actual model is unknown.
- Resolve missing requirements in the main context. If a worker demonstrates
  a capability limit, give it a concrete correction or smaller scope; escalate
  the difficult slice to a stronger model when justified. Record why instead
  of silently upgrading every worker or retrying an unchanged assignment.

### Worktree And Task Ownership

Before launching a task, record in the ExecPlan:

- task ID, deliverable, owned paths, dependencies, and acceptance checks
- worker worktree/branch, explicit remote base ref, and resolved starting commit
- implementation model, effort, and eventual result commit and integration state

Give each concurrent implementation agent its **own worktree and branch**.
The workspace guard selects the integration checkout; it does not create worker
isolation. Main creates worker worktrees from explicit starting commits using
native worktree facilities when available, otherwise Git, following repo rules.
Workers must not edit the integration checkout, another worker's checkout, or
the canonical work item. Separate directories do not remove semantic conflicts:
assign shared interfaces, generated files, lockfiles, and migrations to one owner
or serialize them. Settle shared contracts before dependent work begins.

Seed each task from the recorded integration revision containing its completed
dependencies. Start independent tasks from the same known revision; dependent
tasks wait until prerequisites are integrated. Never start a dependent worker
from stale remote main merely because its worktree is new. Follow the repository's
explicit-ref rules when creating branches/worktrees and transferring commits.

Provide a compact launch packet: absolute worker path and branch, base commit,
task scope and owned paths, approved decisions, dependency contracts, relevant
source, validation commands, allowed mutations, and required result evidence.
The worker must read the actual source before editing. Supply a task-local work
item under its own ignored `.agent/work/` directory. Its `execplan.md` contains
only the assigned task's milestones, owned paths, dependencies, and acceptance
checks; its `decision.md` preserves the relevant approved constraints. Never
copy the full parent execution plan into a worker's executable task plan.
Reference canonical parent artifacts as read-only context. A worker may use
`$implement-execplan` against its **task-local** work item when its required
`.agent/PLANS.md` is available there; otherwise execute the task brief directly.
Never point that skill at the shared work item. Resolve skill paths before
launch and name the exact path in the packet.

Retain a worker for its task's fixes while that context remains useful. Schedule
ready tasks within the runtime's concurrency limit, leaving capacity for main
coordination. Retire finished contexts using runtime facilities when needed;
launch new workers only for ready work or a justified replacement. Workers do
not spawn nested teams, activate goals, change approved intent, or publish.

## 4. Integrate And Validate

Workers implement and validate their assigned behavior, keeping side effects
visible and avoiding speculative abstractions or compatibility scaffolding.
Preserve compatibility only when the user, public contracts, production data,
or rollout requirements demand it. Use `$tdd` when behavioral tests benefit
from it; do not create test seams or conformance tests that mirror the code.

Each worker returns its branch/starting commit and result commit(s), changed
paths, validation commands/results, discoveries, plan deviations, decision-log
entries, and blockers or residual risks. Use local commits for transfer when
repository policy permits. If commits are prohibited, return
an explicit patch including new files. Worker completion means the assigned
slice is ready for integration, not that the parent feature is complete.

The main agent inspects the diff and evidence, integrates results in dependency
order, and owns conflict resolution. Check that each result descends from its
recorded **starting commit**, not just the remote base ref, and stays within its
assignment before applying it. Reconcile cross-task interfaces and generated
outputs; delegate bounded repair to a worker at the updated integration revision
when useful. Never let a
worker merge concurrent results into the shared integration checkout.

For repairs after integration, retain useful worker context but assign a fresh
branch/worktree at the current integration commit containing its prior result.
Record that new starting commit and integrate only new commits after it; do not
reapply the earlier result. Preserve prior worker state until transfer is verified.

Run relevant integration checks on the combined result. Worker test passes do
not establish integrated correctness. Run required repository gates after
focused checks; broaden or repeat testing only for changed code, failures, or
unresolved concerns. Keep actual execution evidence separate from proxy checks.

Fold returned discoveries, deviations, and decisions into the canonical plan;
main checks them against approved intent before accepting the task. Route any
material intent change through approval. Update progress and integration state.
On resume, inspect recorded worktrees, commits, Git status, validation, and
dependencies before scheduling. Preserve finished work; do not replay completed tasks or
reapply already-integrated commits. If a worker context is lost, reconstruct
its remaining assignment from artifacts and source rather than restarting the
whole feature. Record concrete blockers and continue independent ready work.

Activate a native Goal only when the user explicitly requested one and the
runtime supports it. Reference this work item; the main agent still coordinates
execution and acceptance. Do not create, replace, or duplicate a Goal merely
to continue an ordinary implementation request.

## 5. Review The Integrated Result

Run one fresh `$adversarial-review` on the integrated implementation covering
correctness, design clarity, scope, error handling, tests, and adversarial risks.
Use its provider-selection and evidence contract, including cross-provider
review when available. This combines normal closeout and adversarial review;
do not additionally invoke `$review-recent-work`. An explicitly requested formal
`$code-review` may fill this review only if it meets the same independent,
read-only review contract and covers the integrated scope.

Give the reviewer the approved intent, relevant source and adjacent paths, full
change range including new files, task integration record, validation evidence,
and applicable ADR/domain/frontend concerns. Use a fresh context with no inherited
authoring conversation (`fork_turns: "none"` where supported); do not substitute
the author's conclusions for raw evidence. Reviewer reports; main verifies
claims; implementation owners fix; main integrates and validates the fixes.

Follow the review skill's evidence-based stopping rule. A focused follow-up is
warranted when changes introduce a new material risk or invalidate reviewed
evidence. Record the reason and changed surface. Routine corrections need
relevant validation, not another full review for reassurance.

Mark the canonical implementation completed only after planned behavior,
integration validation, and verified review findings are satisfied or explicitly
dispositioned. Report unavailable review capacity or validation honestly; obtain
user judgment when a material acceptance decision remains unresolved.

## 6. Explain And Hand Off

Explain the final feature lifecycle from its entry point, key decisions, source
locations, validation, and remaining limitations. Scale detail to the change.
Use `$explain-implementation` for substantial ownership transfers or when the
user requests a rich walkthrough. An interactive `$lavish` session is optional
unless explicitly requested; serving, rendering, and browser mechanics belong
to the briefing skill.

A missing briefing tool does not undo verified code completion. Report any
incomplete requested handoff separately and provide the best available
source-backed explanation. Preserve local briefing/worktree artifacts until
the user no longer needs them; do not clean up worker state before integration
and evidence have been verified.

Return the outcome, meaningful validation and review evidence, unresolved risks,
and relevant source/work-item/briefing links. State actual model routing and
parallelism when relevant, especially any fallback. Do not dump a phase ledger
or a list of every optional skill that was unavailable.

## Supporting Skill Resolution

Use the registered skill or read its `SKILL.md` from the installed catalog;
user-owned Codex skills live under `~/.agents/skills`. Resolve its references
against its own directory. Read only the lenses needed for the current work.
If a supporting skill is unavailable, carry out this workflow's contract
directly where possible and disclose any material loss of capability. Keep
these same owners and gates when working without subagents; no duplicate
fallback workflow is needed.
