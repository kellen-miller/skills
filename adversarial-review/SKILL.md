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

Run one fresh reviewer against a compact artifact packet for the boundary the
caller selected. Use a reviewer from a different provider than the author
whenever a suitable native or external mechanism is available. Cross-provider
review is the default requirement, not a soft preference. Keep the reviewer
read-only: the main agent verifies each claim against repository or runtime
evidence, directs implementers to fix valid findings, integrates the changes,
and accepts the result.

Before plan approval, use a planning packet only when architecture, security,
data, migration, or other consequential design risk warrants critique; UI scope
alone does not require one. One completed review is the default for a selected
boundary. A
focused follow-up is allowed only when concrete changed evidence introduces
new risk or invalidates the prior conclusion; review that changed surface and
new evidence instead of repeating the full review. Do not spawn nested
reviewers by default. Critical risk may increase capability or reasoning
effort, but does not by itself add reviewers.

## Reviewer Selection

1. Determine the author provider when exposed by the runtime.
2. Discover native provider-aware subagents, then read
   `references/provider-reviewers.md` and inventory its installed external
   adapters before selecting a fallback. Absence from the native subagent
   picker does not establish that another provider is unavailable.
3. Use a suitable reviewer from a different provider when either a native or
   external mechanism is available.
4. Use a fresh isolated session from the same provider only after recording
   that no suitable different-provider mechanism exists or that an available
   mechanism failed its bounded availability check or review invocation.
5. Otherwise use another fresh reviewer available in the current provider and
   record reduced independence.
6. Use the current authoring session only as a last resort.

Do not launch a same-provider reviewer before completing and recording the
cross-provider inventory. If one is launched prematurely, stop it before it
returns a completed status and continue with the cross-provider path; an
interrupted launch does not consume the selected review event.

Record unavailable evidence or provider capacity. Missing cross-provider
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

Treat every finding as a claim. The main agent verifies it, directs
implementers to fix valid bounded issues, updates planning artifacts when
intent or risk changes, rejects unsupported compatibility requests, integrates
the verified changes, reruns relevant validation, and records the disposition.
Request a focused follow-up only under the changed-evidence rule in Core
Contract; do not repeat a full review merely because findings were critical or
the implementer made a fix.
