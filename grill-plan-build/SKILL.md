---
name: grill-plan-build
description: >-
  Use when the user requests grill-plan-build, their full grill/plan/build
  workflow, or a complex feature, refactor, migration, or architecture change
  requiring resolved design decisions and a durable implementation plan.
  Do not use for tiny edits or when the user wants only an answer or review.
---

# Grill Plan Build

Resolve intent, investigate, create and improve an ExecPlan, derive its task
graph, and activate a Goal for execution. Dispatch bounded implementation
workers, validate and independently review the integrated result, then explain
it visually in the PR. The main agent owns user decisions, design, canonical
artifacts, Goal lifecycle, integration policy, and acceptance. Workers own
explicit assignments.

User instructions and existing authorization take precedence. Reuse approved
scope and decisions; ask only about unresolved judgments or material changes.
Phase changes, worker changes, and routine implementation choices do not require
new approval. Workflow invocation does not authorize deployment or unrelated
external writes.

## 1. Establish Scope And Workspace

Inspect relevant source, repository instructions, Git state, and applicable
ADRs. Distinguish unresolved design from a large but already settled change.
For planning that spans sessions because major decisions remain unresolved,
propose a destination and ask the user to invoke `$wayfinder`. Reuse active or
completed maps and their decisions rather than starting a competing plan.

Select an isolated integration worktree, reusing the current managed worktree
when appropriate. `$using-git-worktrees` can assist when available. Honor an
explicit in-place instruction and preserve unrelated changes. Record branch,
explicit remote base ref, resolved starting commit, and actual upstream. Follow
repository branch rules; never infer push safety from the starting branch.

## 2. Grill And Investigate

Keep the user interview and final design with the main agent. Read `$grill-me`
for unresolved intent, tradeoffs, failure behavior, and scope; reuse existing
specs and decision ledgers instead of re-grilling accepted decisions.

Delegate independent, bounded investigations when they materially reduce
planning uncertainty: trace a subsystem, inspect an external contract, find
validation seams, or map shared state. Give each investigator a concrete
question and evidence scope. It returns source pointers, findings, uncertainties,
and suggested boundaries; it does not interview the user or decide scope.

Persist useful research under the ignored work item, with one owner per note.
Investigators inspect source without editing product code. Main reconciles
conflicting findings and validates assumptions before dependent design work.
Do not create a second planning orchestrator or parallelize tightly coupled
questions merely to fill slots.

Apply `$domain-modeling` for terminology, lifecycle, ownership, and durable ADRs;
`$codebase-design` for module boundaries; `$frontend-design` for UI behavior and
rendered evidence. `$grill-with-docs` may combine domain modeling and grilling.
Promote only durable architectural tradeoffs to ADRs; keep delivery decisions
in the plan. Resolve supporting skill paths before delegation.

## 3. Plan And Decompose

Verify `.agent/work/` is Git-ignored before writing canonical artifacts:

```text
.agent/work/<slug>/
  decision.md       # user decisions, assumptions, non-goals, approval provenance
  execplan.md       # final behavior, architecture, task index, progress, acceptance
  meta.json        # stage/state and artifact paths
  research/        # useful source-backed investigation notes, when needed
  tasks/<id>.md    # one bounded executable brief per implementation task
  tickets.json     # dispatcher graph when using scripted execution
```

Keep one parent ExecPlan and one task index. The main agent invokes the planning
skills in this order before implementation:

1. Write `decision.md`, distinguishing user choices from recommendations, and
   initialize `meta.json` with `stage="decision"`, `state="completed"` once
   intent is resolved.
2. Invoke `$execplan-create` for this explicit work item. Follow its repository
   `.agent/PLANS.md` contract, including installing its template when absent.
3. Invoke `$execplan-improve` once on the resulting plan, grounded in actual
   source, interfaces, dependencies, and acceptance checks. Honor its `skip`
   result; repeat only for material discoveries or a user-requested pass count.
4. Derive and validate the task graph below from that improved plan. Reconcile
   any decomposition findings into the same ExecPlan before approval.

Keep the ExecPlan's Progress, Surprises & Discoveries, Decision Log, and Outcomes
& Retrospective current during execution. Use `stage="plan"`, then
`stage="implementation"`; record evidence-backed state. A planning skill's
`state="completed"` means its artifact is ready, not that the user approved it
or execution may begin. On resume, reuse an existing approved ExecPlan and graph;
do not recreate or re-improve them merely because the context changed.

### Make Tasks Executable

Split into independently demonstrable or verifiable behavior slices, sized for
one fresh worker context. Prefer a narrow end-to-end path over backend/frontend/
tests as separate tickets. Refactors may instead follow a coherent seam or an
expand/migrate/contract sequence when that keeps intermediate states usable.
Use one task when splitting would add coordination without independent progress.

Each task records:

- stable ID, concrete deliverable, and observable acceptance checks
- prerequisites by task ID, with the contract each dependency supplies
- owned paths and any shared interface, generated-file, lockfile, or migration owner
- approved constraints, relevant source/research pointers, and validation commands
- status, assigned worktree/branch, actual starting commit, and returned evidence

A task is ready only when prerequisites are integrated and verified, interfaces
are settled, ownership is non-conflicting, and its brief is sufficient to execute.
Check the graph for unknown dependencies, cycles, unnecessary blocking edges,
and tasks too broad for one context. Explain which tasks can run together and
which must serialize. A worker's executable brief contains only its assignment;
the parent plan and shared decisions are read-only pointers.

`$to-tickets` is an alternative for explicitly requested tracker publication,
not a prerequisite for a local graph. Do not publish issues merely to distribute
work. The dispatcher graph is an executable projection of the approved task index;
keep the projection consistent before execution. Once a dispatcher run exists,
its graph and briefs are immutable. For changed scope or repairs, create a new
graph for remaining work at the verified integration revision; preserve prior
run evidence and never replay completed tasks. Update the canonical task index
to point to the new graph rather than maintaining competing plans.

For consequential architecture, auth, billing, data loss, migrations, recovery,
or unresolved design risk, use `$adversarial-review` on the concrete plan before
final approval. Main verifies findings and revises it. Ordinary bounded work
needs source-grounded planning, without mandatory extra critique. Present scope,
implementation split, risks, acceptance, and proposed Goal activation together
for approval. Existing approval suffices for the design it covers; reuse explicit
Goal authorization too. For work that includes Goal activation, design approval
alone is not Goal authorization: if the user has not requested or approved
activation, obtain that permission without reopening settled design choices.
Planning-only, draft-only, and no-Goal requests retain their boundaries.

## 4. Create Goal, Dispatch And Integrate

### Launch Through Goalcraft

After plan approval and explicit Goal authorization, the main agent invokes
`$goalcraft` to create and activate the execution Goal. This is a standard phase
of the full workflow, not an optional helper based on estimated task difficulty.
For planning-only, draft-only, or no-Goal work, skip activation; prepare a Goal
draft only when requested. Do not infer authorization from implicit skill
selection or silence.

Give Goalcraft a compact brief referencing the absolute work-item path,
`decision.md`, `execplan.md`, task index, and dispatcher graph when present.
The objective directs the main agent to execute the approved ExecPlan through
that graph, preserve constraints, reconcile progress, diagnose failures, and
continue through integrated validation, independent review, and the authorized
handoff. Completion requires that evidence, not merely successful worker exits
or a finished dispatcher run. Let Goalcraft validate the objective and activate
it through the available native Goal surface; record its identifier when exposed.

The Goal supplies persistence and completion criteria. The task graph supplies
dependency order; the selected executor below owns scheduling and Git transfer.
Do not launch another executor from Goalcraft or give workers separate Goals.
On resume, inspect and reuse a matching unfinished Goal. Do not replace an
unrelated unfinished Goal without explicit authorization; report the conflict
and retain the validated objective. Follow the runtime's Goal status rules.
If native Goals are unavailable or activation fails, report that limitation,
retain the validated objective, and continue already-authorized implementation
unless the user required active Goal execution. Never claim activation succeeded
without tool evidence.

Prefer `$dispatch-tickets` for repeated ticket distribution. Its script validates
the task graph, launches ready workers within capacity, persists scheduling and
integration evidence, and serializes integration. Use an explicitly configured
worker command and model supported by the runtime. Start with its read-only
preview. Commit the approved source baseline before running its isolated workers.

The script owns routine dispatch and Git transfer; the main agent owns design
changes, failure diagnosis, integration acceptance, and user communication.
Monitor progress without replaying the scheduling algorithm in prose. Do not
run `$implement-spec` or `$grillcraft` as another orchestrator over the same graph.
`implement-spec` remains a user-invoked alternative when scripted execution is
not wanted, not a nested dispatcher.

When the user selects desktop-native subagents or no CLI runner is available, retain
the same graph and responsibilities. Main assigns only ready tasks, supplies
absolute workspace/branch/base-commit pointers and task-local briefs, and
integrates one result at a time. Report the actual fallback. Do not claim
scripted scheduling, model overrides, or parallelism that did not occur.

### Model And Workspace Ownership

Keep the user-selected main model for grilling, planning, orchestration, and
acceptance. Implementation defaults to smaller, lower-cost coding models,
including routine fixes and tests. Main selects these automatically from the
runtime's current supported models and configured providers; the user need not
request cheaper workers or choose a model for each task. Honor explicit user
choices, but never inherit the main model merely because it is the default.
Keep model names and versions out of this policy.

Select the least costly available model capable of the bounded assignment,
with suitable reasoning effort. Set that selection explicitly in each CLI
worker command/profile or native spawn request, including per-task overrides.
Verify profiles resolve to the intended model; a prompt asking a large model
to act like a smaller one does not change routing. With native model overrides,
use a fresh context (`fork_turns: "none"` when required).

Reserve larger models for work with a concrete reasoning need: unresolved
architecture, difficult diagnosis, or a task a smaller worker demonstrably
cannot solve after main checks its brief and environment. First resolve or
isolate that uncertainty so routine implementation can stay with smaller
workers. Escalate only the affected task and record the reason; task volume,
parallelism, or one failing check alone does not justify larger workers.

Record selected model/effort separately from runtime-confirmed evidence.
If smaller-model routing is unavailable, disclose the limitation and actual
fallback rather than silently launching large workers or claiming savings.
Continue authorized work within explicit user cost/model constraints.

Each concurrent implementation worker gets its own worktree and branch from
an exact integration commit containing its completed prerequisites. Never
seed dependent work from stale remote main. Assign shared mutable surfaces to
one owner or serialize conflicting tasks, even when separate worktrees exist.
Workers read actual source before editing, use `$tdd` when useful, and validate
observable behavior without test-only indirection or tests mirroring the code.

Workers do not alter canonical artifacts, integrate other workers' results,
change approved scope, spawn competing orchestration teams, or publish. Git
commit ownership follows the selected executor: the scripted loop commits and
merges; native workers may return local commits or an explicit patch including
new files. Never mix those transfer protocols within one task.

### Accept Results And Handle Failure

Verify changed paths, starting-commit ancestry, validation output, and returned
risks before accepting a result. Integrate in dependency order and run checks
on the combined source; worker test passes do not prove integrated correctness.
Only verified integration unblocks dependents. Update the canonical task index
from executor evidence.

On failure, retain logs and worktrees. The scripted executor stops its workers;
main diagnoses and reconciles state before restarting it. Pause dependent tasks;
resume independent work only when supported by the executor and when the failure
does not invalidate shared assumptions. Never edit run state to bypass failure.
Main resolves scope/contract issues; workers receive bounded fixes against the
current integration revision. Never blindly retry an unchanged failure or
reapply an integrated result. Resume by reconciling Git and durable state first.
Reconstruct lost worker context from its remaining brief rather than restarting
the parent feature. Repairs to integrated work are new tasks at the current tip.

## 5. Review And Explain In The PR

Run one fresh `$adversarial-review` on the integrated implementation, covering
correctness, design clarity, scope, error handling, validation, and applicable
risk. Follow its provider-selection and evidence contract. An explicitly
requested `$code-review` may fill this event if it meets the same independent,
read-only contract. Do not add a separate ordinary closeout review.

Provide approved intent, raw source/diff including new files, task integration
records, validation evidence, and relevant domain/frontend concerns. Main
verifies findings, assigns bounded fixes, and validates the resulting source.
A focused follow-up is warranted only when changed evidence introduces material
risk or invalidates reviewed evidence; no full-review loop for reassurance.

Use `$explain-implementation` and `$pr` to make the PR the visual handoff:
smallest useful lifecycle diagram or diff sketch, consequential decisions,
source pointers, before/after execution evidence, and merge danger. For larger
changes include where a maintainer would change the feature next. Publish only
when authorized; otherwise return the draft body in chat. Explanation gaps do
not undo verified implementation completion.

Mark the work item and its active Goal completed only when approved behavior,
integrated checks, verified review findings, and the authorized handoff are
satisfied or explicitly dispositioned. Return the PR link, meaningful validation,
actual execution/model routing, Goal outcome, and material limitations.
Keep `$retro` user-invoked; do not automatically mutate steering files after
finishing the feature. Preserve worker evidence until transfer is verified.

## Supporting Skill Resolution

Read registered supporting skills from their installed paths; personal skills
live under `~/.agents/skills`. Resolve references relative to each skill. If a
supporting capability is unavailable, carry out this contract directly where
possible and disclose the material limitation, retaining the same owners and
acceptance boundaries.
