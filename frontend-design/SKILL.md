---
name: frontend-design
description: Use when frontend UI, visual design, layout, CSS, responsive behavior, component composition, design-system fit, or frontend implementation quality materially affects the work.
---

# Frontend Design

## Ownership

The current implementation agent owns frontend mutation, integration, browser
evidence, validation, and the decision to accept verified findings. Do not
delegate mutation merely because an independent reviewer exists.

An independent frontend reviewer is optional and follows the parent risk budget.
When `grill-plan-build` selects frontend review, that review satisfies the parent workflow's selected independent-review checkpoint.
Do not run a second independent review under a generic adversarial label.

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

Invoke `$adversarial-review` with a frontend-specific packet only when the
parent risk budget selected planning as the independent checkpoint.

Accepted recommendations must be reflected in `decision.md` or `execplan.md`.
Rejected recommendations must be ignored; do not carry them forward as noise.

## Implementation Pass

The current implementation agent writes the frontend code directly, uses the
existing design system and tokens, implements applicable loading, empty, error,
disabled, permission, mobile, and dense-data states, and gathers evidence after
editing.

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

When the parent risk budget selects implementation review, gather the diff,
planning artifacts, rendered evidence, component and token files, and
validation output. Invoke `$adversarial-review` once with a frontend-specific
lens. Use its provider-selection and status contract. The implementation owner
verifies every finding before changing code.

Ask the reviewer to look for serious issues in:

- visual hierarchy, density, spacing, typography, and responsive behavior
- missing UI states or accessibility regressions
- component boundaries, duplicated styling, token misuse, and brittle CSS
- mismatch between plan, implementation, screenshots, and validation evidence
- unnecessary backwards compatibility or legacy UI noise

Resolve verified critical and high findings before finalization. Fix bounded
medium and low findings when they improve the result; otherwise record them as
residual risk.

Do not preserve backwards compatibility unless the plan explicitly requires it.
