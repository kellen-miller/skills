---
name: grill-plan-build
description: >-
  Use when the user requests grill-plan-build, their full grill/plan/build
  workflow, or a complex feature, refactor, migration, or architecture change
  requiring resolved design decisions and a durable implementation plan.
  Do not use for tiny edits or when the user wants only an answer or review.
---

# Grill Plan Build

Resolve intent, investigate, decompose the plan, dispatch bounded implementation
workers, validate and independently review the integrated result, then explain
it visually in the PR. The main agent owns user decisions, design, canonical
artifacts, integration policy, and acceptance. Workers own explicit assignments.

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

Keep one parent plan and one task index. Follow `.agent/PLANS.md` when present.
Use `stage="plan"`, then `stage="implementation"`; record evidence-backed
`state="active"`, `"blocked"`, or `"completed"`. Decision records distinguish
user choices from recommendations. `$execplan-create` can assist with the repo's
format; `$execplan-improve` addresses concrete uncertainty, not a pass quota.

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
keep it consistent when scope changes, rather than maintaining competing plans.

For consequential architecture, auth, billing, data loss, migrations, recovery,
or unresolved design risk, use `$adversarial-review` on the concrete plan before
final approval. Main verifies findings and revises it. Ordinary bounded work
needs source-grounded planning, without mandatory extra critique. Present scope,
implementation split, risks, and acceptance together for approval; existing
approval suffices when it covers this design.

## 4. Dispatch And Integrate

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

Keep the user-selected main model for design and acceptance. Honor explicit
worker choices; otherwise use a runtime-supported smaller coding model for
bounded tasks and disclose the selection. Research or difficult slices may
need a stronger model. Do not hardcode stale model IDs. With native model
overrides, use a fresh context (`fork_turns: "none"` when required). Record
requested model/effort separately from runtime-confirmed evidence.

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

On failure, retain logs and worktrees, pause dependent tasks, and continue
independent work only when the failure does not invalidate shared assumptions.
Main resolves scope/contract issues; workers receive bounded fixes against the
current integration revision. Never blindly retry an unchanged failure or
reapply an integrated result. Resume by reconciling Git and durable state first.
Reconstruct lost worker context from its remaining brief rather than restarting
the parent feature. Repairs to integrated work are new tasks at the current tip.

Activate a native Goal only when explicitly requested. Do not create a Goal
merely to persist or continue this workflow.

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

Mark completed only when approved behavior, integrated checks, and verified
review findings are satisfied or explicitly dispositioned. Return the PR link,
meaningful validation, actual execution/model routing, and material limitations.
Keep `$retro` user-invoked; do not automatically mutate steering files after
finishing the feature. Preserve worker evidence until transfer is verified.

## Supporting Skill Resolution

Read registered supporting skills from their installed paths; personal skills
live under `~/.agents/skills`. Resolve references relative to each skill. If a
supporting capability is unavailable, carry out this contract directly where
possible and disclose the material limitation, retaining the same owners and
acceptance boundaries.
