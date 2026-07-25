---
name: adversarial-review
description: >-
  Use when plans, implementation diffs, docs, migrations, tests, or other work
  products need an independent critique before execution, finalization, or
  acceptance; especially for complex, high-risk, security-sensitive,
  production, data, billing, auth, or architecture changes.
---

# Adversarial Review

## Core Contract

Run one fresh reviewer against a compact artifact packet. Prefer a reviewer
from a different provider than the author. The reviewer finds serious risks;
the calling agent verifies each claim against repository or runtime evidence
before changing plans, code, or status.

One invocation evaluates one completed planning or implementation boundary. Run one fresh reviewer.
Do not spawn nested reviewers by default. Critical risk may increase the
reviewer's capability or reasoning effort, but it does not add reviewers or
another invocation for the same boundary.

## Reviewer Selection

1. Determine the author provider when exposed by the runtime.
2. Discover native provider-aware subagents and installed reviewer mechanisms.
3. Prefer a suitable reviewer from a different provider.
4. Otherwise use a fresh isolated session from the same provider.
5. Otherwise use another fresh reviewer available in the current provider and
   record reduced independence.
6. Use the current authoring session only as a last resort.

Read `references/provider-reviewers.md` only when an external adapter is
needed. Record unavailable evidence or provider capacity. Missing cross-provider
capacity is not automatically blocking for standard or elevated work; the
caller decides whether critical work requires a waiver.

## Scope And Inputs

Start with the selected worktree, work item, diff, validation evidence, known
risks, and explicit compatibility requirements. Expand to adjacent repositories,
web, browser, or MCP evidence only when a concrete claim requires it, and record
the expansion. Keep the review read-only.

Give the reviewer a compact launch packet:

- goal, constraints, non-goals, and success criteria
- paths to produced artifacts
- relevant diffs, commits, or `git status --short`
- validation commands and results
- known open questions, skipped checks, and residual risks
- explicit compatibility requirements, if any
- for implementation review, the planning artifacts and prior review findings

Store output near the work item when possible:

```text
.agent/work/<slug>/adversarial/plan-review.md
.agent/work/<slug>/adversarial/implementation-review.md
```

If there is no work item, store it under `.agent/adversarial/` or return it
inline when the repository has no suitable artifact directory.

## Compatibility Policy

Backwards compatibility is not a default virtue. Do not ask reviewers to
preserve old names, legacy output shapes, aliases, shims, dual paths, migration
scaffolding, or deprecation wrappers unless the review packet names an explicit
public contract, production data constraint, rollout requirement, or user
instruction. Flag unnecessary compatibility scaffolding as code noise when it
makes the result worse.

## Findings Contract

Report only issues that could change the plan, implementation, validation, or
release decision. Include severity, artifact/path, evidence, impact, and the
next fix or check. Do not reward issue count and do not report nits merely to
produce findings.

End with:

```text
---ADVERSARIAL_REVIEW_STATUS---
AUTHOR_PROVIDER: <provider | unknown>
REVIEWER_PROVIDER: <provider | unknown>
REVIEWER_MODEL: <model | unknown>
INDEPENDENCE: cross_provider | fresh_same_provider | fresh_unknown | current_session
ISSUES_FOUND: <number>
CRITICAL_COUNT: <number>
HIGH_COUNT: <number>
MEDIUM_COUNT: <number>
LOW_COUNT: <number>
CONFIDENCE: HIGH | MEDIUM | LOW
BLOCKING: true | false
SUMMARY: <one line>
---END_ADVERSARIAL_REVIEW_STATUS---
```

The review prompt must prohibit file mutations, external-system changes,
pushes, pull requests, and state-changing commands. It must limit Bash to
inspection commands. Confirm the saved review contains the exact status
delimiter before accepting it. Keep available streaming output with the review
when possible as an audit trail.

## Handling Findings

Treat every finding as a claim. Verify it, fix valid bounded issues, update
planning artifacts when intent or risk changes, reject unsupported compatibility
requests, rerun relevant validation, and return control without re-invoking adversarial review for that boundary.
This remains true after critical or high findings and after material changes to
the reviewed surface.
