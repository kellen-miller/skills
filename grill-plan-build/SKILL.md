---
name: grill-plan-build
description: >-
  Use when the user says "use the full workflow", "run my workflow", "grill
  plan build", "grill then build", or asks to encode a complex feature,
  refactor, migration, architecture change, or security-sensitive change that
  needs artifact-backed planning or effort-shape routing before implementation.
  Do not use for tiny edits, one-line fixes, or answers where the user clearly
  wants no planning workflow.
---

# Grill Plan Build

## Core Contract

Run an artifact-backed implementation workflow. Do not start by coding. First
extract intent, then write durable artifacts, then execute through an
evidence-checked plan and goal loop.

Optimize for the best current shape of the system. Do not preserve backwards
compatibility, legacy shims, aliases, dual paths, deprecation scaffolding, or
old output shapes unless the user, a public contract, production data, or a
rollout plan makes that compatibility requirement explicit.

This skill orchestrates these skills when available:

- `$grill-me`
- `$grillcraft`
- `$execplan-create`
- `$execplan-improve`
- `$goalcraft`
- `$implement-execplan`
- `$review-recent-work`
- `$adversarial-review`
- `$using-git-worktrees`

It may also use these skills as phase-specific lenses when they fit:

- `$grill-with-docs`
- `$domain-modeling`
- `$codebase-design`
- `$tdd`
- `$code-review`
- `$frontend-design`

`$wayfinder` is a user-invoked on-ramp, not an orchestrated sub-skill or a
phase-specific lens. Hand work to it only through the effort-shape gate below.

## Sub-Skill Resolution

Every `$skill` reference resolves in this order:

1. If the name is registered as an invocable skill in the current runtime,
   invoke it through the native skill mechanism.
2. Otherwise resolve it to a `SKILL.md` on disk and execute that file inline:
   read it and follow its instructions as written. The personal skill root is
   `~/.agents/skills/`; installed sub-skills live under it, typically at
   `_managed/<name>/SKILL.md` (for example
   `_managed/grill-me/SKILL.md` and `_managed/using-git-worktrees/SKILL.md`).
   Match by the target's frontmatter `name`, not by directory name alone.
3. Resolve any paths a sub-skill references against that sub-skill's own
   directory, and inherit its declared tool and write constraints.
4. If no matching `SKILL.md` exists, treat the lens as unavailable, continue the
   core workflow, and note the skipped lens in the final output.

When a phase runs in a subagent, pass the resolved absolute `SKILL.md` path in
the launch packet and require the subagent to read and execute it in its fresh
context. File-path execution satisfies the phase contract; it does not relax the
Fresh-Context Launch Contract or the fixed review lifecycle.

## Skill Routing

Use the smallest set of supporting skills that improves the work.

- Wayfinder handoff: when planning itself is too large for one session and the
  route is obscured by unresolved decisions or investigations, stop this
  workflow and tell the user to invoke `$wayfinder` with a proposed destination.
  Do not invoke it automatically.
- Wayfinder continuation: when an active Wayfinder map still has open child
  tickets or in-scope fog under `Not yet specified`, stop this workflow and tell
  the user to invoke `$wayfinder` with the map URL. Never resolve more than one
  Wayfinder ticket per session.
- Wayfinder re-entry: when a Wayfinder map has no open child tickets and no
  remaining in-scope fog, treat its destination and linked resolutions as
  settled planning input and continue this workflow without re-grilling them.
- Default grill: invoke `$grill-me`.
- Docs-backed grill: invoke `$grill-with-docs` instead of `$grill-me` when the
  work should update `CONTEXT.md`, create or refine ADRs, or settle shared
  domain language.
- Domain lens: use `$domain-modeling` when terms, lifecycle states, ownership,
  or domain boundaries are fuzzy. Capture resolved terms in `CONTEXT.md` and
  create ADRs only for hard-to-reverse, surprising, trade-off-heavy decisions.
- Design lens: use `$codebase-design` during ExecPlan improvement when the work
  changes module shape, public interfaces, seams, adapters, or testability.
- Frontend design lens: use `$frontend-design` when frontend UI, visual design,
  layout, CSS, component composition, responsive behavior, or design-system fit
  materially affects the result. At each adversarial boundary, a
  frontend-specific packet consumes that boundary's one event instead of
  adding another generic adversarial review.
- TDD lens: use `$tdd` during implementation slices when behavior can be
  verified at an agreed seam.
- Formal review: use `$code-review` only when there is both a fixed point such
  as `main`, a merge-base, or a commit, and a spec source such as `decision.md`,
  `execplan.md`, a PRD, or an issue. Otherwise select `$review-recent-work`.
- Shape over compatibility: treat backwards-compatibility concerns as invalid
  unless backed by an explicit requirement. Prefer removing old names, shims,
  wrappers, and compatibility paths when they would make the new shape noisier.
- Worktree guard: invoke `$using-git-worktrees` before creating `.agent/work`
  artifacts or mutating repository files.

Do not invoke supporting lenses just because they are installed. If a lens is
unavailable, continue with the core workflow and note the skipped lens in the
final output.

## Orchestrator Contract

The main agent is the durable orchestrator and user-facing owner. It owns the
effort-shape gate, risk classification, user questions and approvals, worktree
selection, Goal activation, phase gates, and final acceptance. It dispatches
bounded agents to invoke phase subskills and does not repeat completed phase
work merely for reassurance.

Worktree selection and Goal activation stay with the main agent. Pass every
subagent the absolute worktree path, branch, base ref, explicit work-item path,
selected skill, risk tier, allowed mutations, and required status contract.

### Fresh-Context Launch Contract

Fresh planning and review agents receive only a minimal task-local launch
packet. Do not inherit the orchestrator's conversation when the runtime
supports no-inheritance launch controls; in the current runtime, launch a fresh
agent with `fork_turns: "none"`. Give fresh agents the applicable lenses and
raw artifacts or evidence they need to inspect, not the orchestrator's
conclusions about those artifacts.

The packet may contain the absolute repository and worktree paths, branch and
base ref, explicit work-item or artifact paths, selected skill, risk tier,
applicable lenses, allowed mutations, required output/status contract, and raw
decision ledger, diffs, validation output, or other evidence. It must not
include prior conversational reasoning except where that reasoning is itself a
durable raw artifact the agent must evaluate.

Keep the persistent grill and implementation agents on their existing contexts:
the grill agent carries user decisions across questions and the implementation
agent carries implementation state across milestones. This no-inheritance rule
applies to fresh planning and review agents, not those persistent agents.

Use these phase agents when subagents are available:

- one persistent grill agent invoking `$grill-me` or `$grill-with-docs`
- one fresh planning agent invoking `$grillcraft` in planning-only,
  no-activation mode with exactly one `$execplan-improve` attempt by default
- one persistent implementation agent invoking `$implement-execplan`
- one fresh closeout reviewer invoking exactly one normal review skill
- one fresh planning adversarial reviewer invoking `$adversarial-review`
- one fresh implementation adversarial reviewer invoking `$adversarial-review`

Reuse the grill agent across user answers and the implementation agent across
milestones. Always use fresh planning and review contexts. Only one mutating
agent operates in the worktree at a time unless the main agent has explicit
disjoint paths and an integration plan.

If subagents are unavailable, the main agent may invoke the phase skill itself
and must record that context isolation was unavailable.

If a phase agent returns an incomplete status, continue the same agent once
with the missing requirements. Replace it only when its context is corrupted,
it is unavailable, or the retry fails. Stop with an evidence-backed blocker if
the replacement still cannot satisfy the phase contract.

The continuation-or-replacement rule ends when an adversarial reviewer returns
its completed boundary status. Context loss, reviewer unavailability, later
fixes, changed artifacts, and failed validation do not reopen that boundary or
authorize another adversarial-review invocation for it.

## Risk And Review Lifecycle

| Tier | Typical shape | Plan-improvement depth | Planning adversarial review | Normal closeout review | Implementation adversarial review |
| --- | --- | --- | --- | --- | --- |
| Standard | Bounded feature or refactor with ordinary rollback | Exactly one improvement attempt | Exactly one adversarial event | Exactly one closeout event | Exactly one adversarial event |
| Elevated | Architecture, public interface, meaningful production rollout, or difficult recovery | Exactly one improvement attempt | Exactly one adversarial event | Exactly one closeout event | Exactly one adversarial event |
| Critical | Security, authz, billing, credible data loss, destructive migration, or similarly irreversible change | Up to two improvement attempts | Exactly one adversarial event | Exactly one closeout event | Exactly one adversarial event |

Risk changes plan-improvement depth and reviewer capability, not review-event count.
Every tier runs the same three review events in lifecycle order. Findings may
change the work and validation evidence, but they never create another review
event at the completed boundary.

## Independent Reviewer Selection

Use this ordered selection contract for each planning-boundary and
implementation-boundary adversarial review, including the fallback path:

1. Identify the authoring provider when the runtime exposes it.
2. Discover suitable read-only review mechanisms available in the current
   environment and prefer a suitable different-provider reviewer.
3. Otherwise use a fresh isolated session from the same provider.
4. If no isolated reviewer is available, use another fresh reviewer in the
   current provider and record the reduced independence.
5. Use the current authoring session only as a last resort.

Record the author provider, reviewer provider, model when known, independence
level, evidence limitations, and the review severity and status fields in the
work item. For critical work, the main agent decides whether a missing
cross-provider capacity blocks acceptance or requires explicit user waiver.

## Model And Reasoning Profiles

Express profiles by capability and map them to concrete models only when the
runtime supports model selection:

- main orchestrator: user-selected capable model, medium reasoning; high only
  for difficult routing or synthesis
- grill agent: balanced repository-capable model, medium reasoning
- planning agent: strong design and coding model, high reasoning
- implementation agent: strong coding model, medium reasoning; high for a
  concretely difficult slice
- closeout reviewer: balanced model in a fresh context, medium reasoning
- independent reviewer: suitable different-provider model when available,
  medium reasoning; high for critical work

For every supported phase launch, explicitly set both `model` and
`reasoning_effort` to the concrete values selected from these profiles. Do not
leave either value to inheritance when the launch API supports the override.
Record phase evidence with `actual_model`, `actual_reasoning_effort`, and
`unavailable_capability_fallback`; use `none` for the fallback when every
selected capability was available. If the runtime cannot set an override,
record the actual inherited value when observable and name the unavailable
capability and chosen fallback.

Do not prescribe `xhigh` or `max` as a default. Escalate only for concrete
risk, uncertainty, or repeated failed attempts.

## Workflow

### Step -1: Effort Shape Gate

Before creating a worktree, issue, `.agent/work` artifact, or implementation
change, determine whether the route can be planned in one session. Read-only
repository and tracker inspection is allowed during this gate.

| Observable shape | Route |
| --- | --- |
| Bounded uncertainty that one grill can resolve | Continue to Step 0 |
| Large implementation with settled decisions or an approved spec | Continue to Step 0 |
| Planning spans sessions because major decisions or investigations remain unresolved | Hand off to `$wayfinder` |
| Active Wayfinder map with open tickets or in-scope fog | Return to `$wayfinder` |
| Completed Wayfinder map with a clear route | Continue to Step 0 using the map as input |

Do not equate implementation length with planning fog. Many milestones, files,
services, or implementation sessions do not require Wayfinder when the route is
already clear enough to write an executable plan.

For a Wayfinder handoff, stop before Step 0 and return:

- current phase: `wayfinder-handoff`
- why the route cannot yet be planned in one session
- a concise proposed destination, or the active map title and URL
- the exact user invocation: `$wayfinder <proposed destination>` or
  `$wayfinder <map URL>`

Wayfinder is planning by default and user-invoked. Do not create its map, claim
its tickets, or proceed into `grill-plan-build` artifacts or implementation as
part of the handoff.

### Step 0: Worktree Guard

Before writing any artifact or implementation change, ensure the agent is
working in an isolated workspace.

- Invoke `$using-git-worktrees` when the task is inside a git repository.
- If already in a linked worktree, record the path, branch, base ref, and
  upstream/tracking state, then continue there.
- If in the primary checkout, create or select an isolated worktree and `cd`
  into it before creating `.agent/work/<slug>/` or editing source files.
- If the primary checkout has unrelated local changes, do not edit it. Move the
  work into an isolated worktree first.
- If the user explicitly requires in-place work, record that exception in
  `decision.md` and `meta.json` before writing files.
- If the task is not in a git repository, record that no worktree can be used
  and keep all artifact paths explicit.

Do not rationalize that planning artifacts are harmless in the primary
checkout. `.agent/work` artifacts, docs, source edits, generated files, and
validation output all belong to the selected worktree for this workflow.

### Step 1: Grill Agent

Spawn one persistent grill agent. Keep it read-only for `$grill-me`; for
`$grill-with-docs`, allow writes only to selected `CONTEXT.md` and ADR paths.
The agent inspects discoverable facts, proposes the next question, and returns
confirmed decisions, open decisions, assumptions, risks, validation, rollout,
and rollback. The main agent asks the user and forwards answers to the same
agent. Do not implement during this phase.

When entering from a completed Wayfinder map, load the destination, `Decisions
so far`, `Not yet specified`, and `Out of scope` sections. Read linked
resolution tickets on demand, preserve settled decisions and scope boundaries,
and grill only unresolved delivery concerns.

### Step 2: Planning Agent

Spawn one fresh planning agent with the completed decision ledger. Tell it to
invoke `$grillcraft` in planning-only and no-activation mode with exactly one
`$execplan-improve` attempt for standard or elevated work, or up to two for
critical work. It writes `decision.md`, `meta.json`, and `execplan.md` in the
selected worktree. Lenses do not create review checkpoints.

The result must be a work item:

```text
.agent/work/<slug>/
  decision.md
  meta.json
  execplan.md
```

`decision.md` is the intent and provenance record. `execplan.md` is the
executable implementation contract. `meta.json` is the lifecycle source of
truth.

When entering from a completed Wayfinder map, add a `Wayfinder provenance`
section to `decision.md` with the map title, URL, and relevant resolved tickets.
Record `"wayfinder_map_url"` and `"wayfinder_state": "completed"` in
`meta.json`; link instead of copying resolved-ticket content.

`decision.md` and `execplan.md` must record, when relevant:

- worktree path, branch, base ref, and any explicit in-place-work exception
- domain terms confirmed or changed
- `CONTEXT.md` updates made or intentionally skipped
- ADRs created or intentionally skipped
- modules, interfaces, seams, and adapters touched
- frontend surfaces, layout decisions, visual states, component paths, design
  tokens, and responsive behavior to preserve or change
- test seams selected for TDD
- validation commands and acceptance criteria
- frontend validation evidence: check/test/build commands, browser smoke checks,
  screenshots, or written browser observations when useful
- compatibility intentionally not preserved, when old behavior or names are
  removed

During improvement, check for shallow wrappers, leaked policy, premature seams,
internal-only tests, speculative abstractions, unjustified compatibility code,
and incomplete frontend design states.

Accept the phase only when the artifacts preserve confirmed decisions, contain
observable validation, and leave `meta.json` at `stage="plan"` and
`state="completed"`. Run exactly one planning-boundary adversarial review after planning is complete,
using the Independent Reviewer Selection contract. If frontend work is in
scope, its frontend-specific packet consumes this event. Verify each finding,
fix or disposition valid findings, update the planning artifacts when needed,
and rerun relevant validation. Never re-invoke the adversarial reviewer for that boundary,
including after critical or high findings. The planning agent never activates
a Goal.

### Step 3: Goal And Implementation Agent

After accepting the planning packet and completing its one adversarial review,
the main agent invokes `$goalcraft`. Then spawn one persistent implementation
agent for the explicit work-item path and tell it to invoke
`$implement-execplan`.

The implementation agent sets active state, implements vertical slices, keeps
the ExecPlan living sections current, validates meaningful slices, records
blockers, and sets completed state only after planned implementation and
validation succeed. Route material discoveries through the main agent before
changing intent.

Before coding, set:

```json
{
  "stage": "implementation",
  "state": "active"
}
```

When blocked, set `stage="implementation"` and `state="blocked"`. When
complete, set `stage="implementation"` and `state="completed"`. Do not mark
complete because of elapsed time, budget exhaustion, partial implementation, or
proxy checks alone.

Use `$tdd` at pre-agreed seams where practical. Do not add speculative
abstractions, compatibility layers, old-name aliases, legacy output adapters,
or migration shims unless `decision.md` records an explicit requirement.

### Step 4: Review Agents

Run exactly one normal closeout review after implementation. The main agent
selects exactly one normal review skill: `$review-recent-work` for workflow
closeout or `$code-review` for an explicitly requested formal branch/PR review
or standards/spec split. They are alternatives, not additive defaults.

Fix or disposition its verified findings and rerun relevant validation through
the persistent implementation agent. The closeout reviewer is not the
implementation adversarial reviewer.

Run exactly one implementation-boundary adversarial review after implementation and normal closeout are complete,
using the Independent Reviewer Selection contract. Review against
`decision.md`, `execplan.md`, worktree and branch state, `git status --short`,
`git diff`, relevant tests, adjacent code paths, and rendered evidence when UI
changed. If frontend work is in scope, its frontend-specific packet consumes
this event and does not add another generic adversarial review.

Fix or disposition its verified findings, rerun relevant validation, and finalize
through the persistent implementation agent. Never re-invoke the adversarial reviewer for that boundary,
including after critical or high findings. Do not ask an adversarial reviewer
to spawn more reviewers; reviewer capability may increase for critical work,
but the event count does not.

## Fallback Path

Run the effort-shape gate before using this fallback. If it produces a
Wayfinder handoff, stop there. Otherwise, when native subagents or
`$grillcraft` are unavailable, the main agent preserves the selected risk tier,
phase order, alternative closeout review selection, one-improvement standard
default, and fixed review lifecycle itself:

1. `$using-git-worktrees`
2. `$grill-me` or `$grill-with-docs` using the Step 1 contract
3. Create `.agent/work/<slug>/decision.md` and `meta.json` with
   `stage="decision"` and `state="completed"`
4. `$execplan-create`, followed by exactly one `$execplan-improve` attempt for
   standard or elevated work, or up to two for critical work
5. Perform exactly one planning-boundary adversarial review, fix or disposition
   verified findings, rerun relevant validation, and do not invoke it again
6. The main agent invokes `$goalcraft`, then `$implement-execplan` using the
   Step 3 lifecycle contract
7. Perform exactly one closeout review with `$review-recent-work` or
   `$code-review`, fix or disposition verified findings, and rerun relevant
   validation
8. Perform exactly one implementation-boundary adversarial review, fix or
   disposition verified findings, rerun relevant validation, and finalize
   without invoking it again

If `/goal` is unavailable in the current Codex surface, use
`$implement-execplan` instead of `$goalcraft` and record the limitation.

Record that context isolation was unavailable and retain all phase-gate and
review evidence in the work item.

## Output

Return:

- current workflow phase
- effort-shape route and Wayfinder handoff or map status, when applicable
- risk tier and the plan-improvement and reviewer-capability profiles used
- worktree path, branch, base ref, and upstream/tracking state
- work item path
- artifacts created or updated
- supporting lenses used or skipped
- normal closeout and both adversarial-review artifacts and outcomes
- current lifecycle state
- validation run
- next action or blocker
