---
name: explain-implementation
description: >-
  Reconstruct a completed implementation from its final code, work-item
  artifacts, tests, and validation evidence, then create a source-backed,
  interactive HTML briefing for the human implementer. Use after implementation
  and review are complete, at the end of grill-plan-build, or when the user asks
  for an implementation tour, ownership guide, code walkthrough, or help
  understanding how a recently built feature works.
---

# Explain Implementation

## Core Contract

Transfer implementation ownership to the human. Reconstruct the final system
from source evidence and render one self-contained interactive briefing at:

```text
.agent/work/<slug>/implementation-briefing.html
```

Treat the page as a local human artifact:

- require the target path to be ignored by Git
- never stage or commit the page
- create no sidecar files in the work item
- keep Git status unchanged
- state that removing the worktree removes the page

The renderer produces the source HTML that Lavish serves and annotates. Lavish
is the required review and ownership interface. The renderer remains the
deterministic content boundary for schema, source, and Git-state validation;
direct-open portability is an artifact property, not an alternate workflow.

Do not write a decorated diff summary. Explain the feature lifecycle from its
main entry point, then progressively expose boundaries, execution paths,
decisions, change recipes, proof, and retrieval questions.

## Input Resolution

Prefer an explicit `.agent/work/<slug>/` path. When invoked directly without
one, select the most recently updated work item whose `meta.json` records
`stage="implementation"` and `state="completed"`.

Require:

- a Git repository
- an ignored `.agent/work/<slug>/` target
- `decision.md`, `execplan.md`, and `meta.json`
- a completed implementation state

If the work item and observable implementation do not overlap, stop and report
the mismatch. Do not explain unrelated recent changes.

## Fresh Reconstruction

When a parent workflow can launch agents, run this skill in one fresh context
with no inherited implementation conversation. Give it:

- repository and worktree paths
- branch and base ref
- explicit work-item path
- final diff or commit evidence
- final validation output
- review findings and their dispositions
- rendered evidence when UI changed

The briefing agent may write only the ignored HTML output and temporary
rendering input. It is not another review event. If reconstruction reveals a
material code or artifact contradiction, return it to the implementation owner,
rerun relevant validation, and regenerate the briefing without reopening
completed review boundaries.

## Reconstruct The Implementation

Read `decision.md`, `execplan.md`, and `meta.json`, then inspect:

- `git status --short`
- the final diff or completed commit
- files named by the work item
- entry points and adjacent callers
- domain types, boundaries, and side effects
- tests that establish behavior
- configuration, migrations, rollout, and operational surfaces
- final validation and review evidence

Prefer final code over planned code. Record plan deviations when they help the
human understand the resulting shape.

Keep side effects visible. Explain network calls, database writes, goroutines,
queues, stream waits, retries, and fallbacks at the step where they occur.

Ground every important claim in a repository-relative source reference with a
path and, when useful, a symbol and line number. Do not invent a source,
invariant, test, failure path, or operational behavior.

## Build The Briefing

Read [references/briefing-schema.md](references/briefing-schema.md) completely.
Create valid schema-version `2` JSON in a temporary OS directory outside the
repository.

The content must provide:

1. A thirty-second orientation with why, before, after, entry point, and key
   concepts.
2. A component map organized by ownership rather than by changed-file list.
3. A happy-path trace and at least one important failure, retry, or edge path
   when one exists.
4. Decisions and tradeoffs that explain the final shape.
5. Change recipes for likely maintenance tasks.
6. Behavioral proof mapped to tests, commands, and observed evidence.
7. At least three retrieval questions, including one failure prediction and one
   "where would you change this?" question.
8. Known limits and a glossary when they add real value.

Use concise source excerpts only when the exact syntax is essential. Prefer
links to source over copied code that will go stale.

Do not claim that the user understands material merely because the page covers
it. The questions expose gaps; they do not create learning records.

## Render

Resolve paths against this skill directory and run:

```bash
python3 scripts/render_briefing.py \
  --input <temporary-briefing.json> \
  --output <repo>/.agent/work/<slug>/implementation-briefing.html \
  --repo-root <repo>
```

The renderer:

- validates the briefing schema and cross-references
- verifies that every source reference resolves to a real repository file
- resolves live branch, HEAD, tree state, and snapshot fingerprint
- requires the exact ignored output location
- embeds all data, CSS, and JavaScript into one HTML file
- fails if rendering changes Git status

Keep the temporary rendering directory only through the ownership session so
feedback can update the evidence model and rerender the same page. Remove it
when the session is complete.

## Lavish Ownership Session

Open the rendered file through Lavish using its current unversioned CLI
invocation with telemetry disabled:

```bash
LAVISH_AXI_TELEMETRY=0 npx -y lavish-axi <briefing-path>
LAVISH_AXI_TELEMETRY=0 npx -y lavish-axi poll <briefing-path> \
  --agent-reply "The briefing is ready; start with the orientation and flow."
```

If `npx -y` cannot run, use only the installed-copy invocations documented by
`$lavish`. If none can start or resume the session, stop with the ownership
phase blocked. Do not substitute a direct-open handoff or chat-only review.

Keep each poll in the foreground. When a parent workflow invoked this skill,
the main agent owns the foreground poll and forwards its result to this same
briefing agent. Do not use `&`, `nohup`, or an unobserved background process.
Never invoke `lavish-axi share`; the implementation evidence remains local.
Keep the server on loopback.

## Validate The Experience

In the Lavish-served browser page, verify:

- no console errors
- the first render explains the feature without interaction
- selecting a component updates its ownership details
- changing flows and stepping forward and backward updates the trace
- a retrieval answer produces correct feedback and source evidence
- keyboard focus and controls work
- the layout remains readable at approximately 390px and 1280px widths
- source links point at real repository files, and Lavish mode exposes a
  working copy-path control when its HTTP iframe cannot open `file://` links
- `git status --short` matches its pre-render state

Fix the skill output or evidence and rerender when validation fails.
Use annotations and structured actions to distinguish:

- an explanation gap: update the temporary evidence model and rerender the same
  HTML path
- a source question: answer with the exact repository reference and improve the
  page when the explanation was insufficient
- a material contradiction: return it to the implementation owner, rerun
  relevant validation, and regenerate the briefing
- a browser `layout_warnings` result: repair and recheck before asking the human
  to continue

Poll again after each feedback batch. A timeout, interruption, or feedback
response does not finish the loop. Stop when the user ends the session, and do
not reopen a user-ended session without an explicit request.

Lavish owns the review loop without becoming part of the generated file. The
single-file artifact boundary remains because it is the input Lavish serves and
the local evidence artifact the worktree owns.

## Return

Report:

- the absolute clickable briefing path
- the work item and source snapshot
- which flows and maintenance recipes it covers
- browser and Git-status validation
- Lavish ownership-session outcome
- any evidence gaps or reconstruction blocker
- that the artifact is local-only and disappears with its worktree
