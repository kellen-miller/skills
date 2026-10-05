---
name: explain-implementation
description: >-
  Use when a completed implementation needs an ownership transfer, a visual
  PR explanation, or a source-backed walkthrough of its lifecycle and decisions.
---

# Explain Implementation

Explain the final implementation visually in the PR body. Start with the
concrete trigger and resulting behavior, then show the lifecycle from its main
entry point. Scale the explanation to the change; a small fix needs little prose.

## Reconstruct From Evidence

Read the approved intent, final diff including new files, entry points and
adjacent callers, relevant tests, execution output, and review dispositions.
Prefer final source over planned source. Explain deviations only when they
help a reviewer understand the resulting design. Use the repository's glossary
and architectural decisions when present.

A fresh explanation context can help for a substantial implementation. Give it
source and evidence pointers, not the author's conclusions. It drafts the PR
body; the main agent verifies it and owns publication. This is explanation,
not another mandatory review event.

## Draft The PR Body

Read `$pr` for its Summary, Evidence, and Merge Danger structure. If unavailable,
use those three sections directly. Respect the repository's PR template.

- **Summary:** choose the smallest visual that explains the change: a call tree,
  before/after diff sketch, component/file tree, pseudocode, or Mermaid. Put
  brief prose next to it. Show ownership and side-effect ordering; include an
  important failure or recovery path when it changes the behavior. For a larger
  ownership transfer, explain consequential decisions and where a maintainer
  would change the feature next. Link verified source paths or symbols.
- **Evidence:** map the claimed behavior to observed before/after output,
  relevant tests, or UI screenshots. Distinguish execution from source inspection
  and simulated checks. Never invent a failing baseline, screenshot, or test run.
  Say when a baseline or requested validation is unavailable.
- **Merge Danger:** explain reversibility (one-way or two-way door), affected
  surfaces, and any rollout, recovery, or remaining limitation that affects review.

Use committed GitHub source links in published PRs; local absolute file links
are for chat. Avoid copying large code blocks that obscure the lifecycle.
Do not add quizzes, HTML artifacts, annotation servers, or a separate briefing
workflow. The PR itself is the durable visual explanation.

## Verify And Publish Within Scope

Check the explanation against the final source, validation, and review evidence.
If reconstruction finds a material contradiction, return it to the implementation
owner for a bounded fix and relevant validation; update the explanation afterward.
Reopen review only when changed evidence introduces material risk.

Create or update the PR only when already authorized. Otherwise return the draft
body in chat; missing publication permission does not block explaining the work.
Report the PR link, meaningful checks, and material limitations. Do not treat
writing an explanation as proof that the implementation is correct.
