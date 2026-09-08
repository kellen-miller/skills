---
name: frontend-design
description: Use when frontend UI, visual design, layout, CSS, responsive behavior, component composition, design-system fit, or frontend implementation quality materially affects the work.
---

# Frontend Design

## Ownership

The main workflow agent owns review selection, integration, verified-finding
disposition, and acceptance. Implementation agents own frontend mutation and
evidence in their assigned worktrees; reviewers remain read-only. Do not assign
integration or acceptance to an implementation or review worker.

When frontend work is in scope, the parent may select a frontend-specific
packet for its planning or implementation review. This lens contributes to
that review and does not create an unconditional or additional review.

## Preconditions

- Work in the selected worktree from the parent workflow.
- Inspect the existing design system, component patterns, CSS strategy, token
  files, layout primitives, and validation commands before changing UI.
- For Svelte or SvelteKit work, use official Svelte docs or the Svelte MCP tools
  when available before changing Svelte-specific patterns.
- For third-party frontend APIs, use Context7 or official documentation before
  relying on library-specific syntax or configuration.
- For rendered UI, DOM, console, network, accessibility, responsive behavior, or
  browser smoke evidence, use Playwright, Chrome DevTools, browser tools, or
  equivalent local validation.

## Use The Lens

Evaluate frontend work for:

- product fit, visual hierarchy, information density, layout, responsive states
- component composition, CSS strategy, design tokens, interaction states
- loading, empty, error, permission, disabled, dense-data, and mobile states
- design-system fit, accessibility, keyboard behavior, and visual polish
- design-to-code direction, implementation risk, and validation evidence

Do not use this skill as the authority for backend behavior, security, data
modeling, or final correctness.

## Planning Pass

The planning agent identifies:

- the strongest UI shape for the feature
- design-system files and components likely to matter
- responsive, empty, loading, error, permission, disabled, and dense-data states
- implementation slices that protect visual coherence
- screenshots, browser observations, or accessibility evidence needed before
  closeout
- validation commands that prove the frontend result

Before plan approval, when the parent selects planning critique for architecture,
security, data, migration, or other consequential design risk, contribute a
frontend-specific packet to that review. UI scope alone does not require
planning critique.

Accepted recommendations must be reflected in `decision.md` or `execplan.md`.
The main workflow agent verifies every finding, directs implementers to fix
valid findings, and reruns relevant validation. Rejected recommendations must
be ignored; do not carry them forward as noise.

## Implementation Pass

Implementation agents write frontend code in their assigned worktrees, use the
existing design system and tokens, implement applicable loading, empty, error,
disabled, permission, mobile, and dense-data states, and gather evidence after
editing. Return the evidence to the main workflow agent for integration and
acceptance.

Before editing:

- read the target components and adjacent component patterns
- read relevant token/theme/layout files
- inspect existing tests and validation scripts
- gather rendered evidence first when the issue is visual or responsive

While editing:

- keep changes inside the planned frontend paths unless the plan changes
- use existing components, icons, layout primitives, and token conventions
- implement expected loading, empty, error, disabled, permission, mobile, and
  dense-data states when they apply
- avoid compatibility shims, old aliases, dual paths, and legacy UI surfaces
  unless the plan records an explicit requirement

After editing, record this status block in the work item or final notes:

```text
---FRONTEND_IMPLEMENTATION_STATUS---
implementer: <agent or provider>
status: changed | no_change | blocked
changed_files:
docs_evidence:
browser_evidence:
validation:
critical:
high:
medium:
low:
```

The pass succeeds only when the diff contains the intended frontend changes and
validation evidence exists for the risk level. If docs or browser evidence were
needed but unavailable, record the gap as a blocker or residual risk.

## Independent Frontend Review

When the parent selects implementation review, gather the diff, planning
artifacts, rendered evidence, component and token files, and validation output.
Contribute a frontend-specific lens to that parent-selected independent review,
covering design, ordinary correctness, validation, and adversarial risks in its
single packet. Use `$adversarial-review`'s provider-selection and status
contract; do not create a separate review for this lens.

The main workflow agent verifies every finding, directs implementers to fix
valid issues, integrates the result, and accepts it. Follow-up review is
allowed only when concrete changed evidence introduces risk or invalidates the
prior conclusion, and must stay focused on the changed surface.

Ask the reviewer to look for serious issues in:

- visual hierarchy, density, spacing, typography, and responsive behavior
- missing UI states or accessibility regressions
- component boundaries, duplicated styling, token misuse, and brittle CSS
- mismatch between plan, implementation, screenshots, and validation evidence
- unnecessary backwards compatibility or legacy UI noise

The main workflow agent fixes or dispositions valid findings through the
implementation agents, reruns relevant validation, and finalizes.

Do not preserve backwards compatibility unless the plan explicitly requires it.
