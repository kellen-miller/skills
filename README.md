---
name: managing-agent-skills
description: >-
  Use when installing, synchronizing, extending, validating, or troubleshooting
  the personal agent skills repository and its authored or managed dependencies.
---

# Agent Skills

This repository is the source of truth for the personal agent skills installed
at `~/.agents/skills`. It contains two kinds of content:

- authored skills maintained directly in this repository; and
- external skills declared in a manifest and materialized by `skillctl`.

The installation directory is intentionally a symlink to the checkout. That
keeps authored files editable, managed dependencies reproducible, and every
agent runtime on the machine pointed at the same skill tree.

## Requirements

- Git
- Python 3.11 or newer
- [`uv`](https://docs.astral.sh/uv/)
- Network access to the Git repositories declared in `deps.yaml` and any
  machine-local overlay

## Install

Clone the repository, link the standard cross-runtime skill directory to it,
and materialize the managed dependencies:

```sh
git clone https://github.com/kellen-miller/skills.git \
  ~/development/github/kellen-miller/skills
ln -s ~/development/github/kellen-miller/skills ~/.agents/skills
cd ~/.agents/skills
uv sync --locked
uv run --locked skillctl sync
```

If `~/.agents/skills` already exists, inspect it before creating the symlink.
Do not replace an existing directory until its authored content and local
configuration have been preserved.

## Repository Layout

| Path | Ownership | Purpose |
| --- | --- | --- |
| `<skill-name>/SKILL.md` | Authored | User-owned skills maintained and reviewed here |
| `_managed/` | Generated | External skills materialized from the dependency manifests |
| `deps.yaml` | Authored | Public, reproducible external dependency declarations |
| `deps.local.yaml` | Machine-local | Ignored additions, extensions, and removals for one machine |
| `src/skillctl/` | Authored | Manifest validation and atomic synchronization implementation |
| `tests/` | Authored | Publication, synchronization, and workflow regression coverage |
| `pyproject.toml` | Authored | Python package, CLI entry point, and development tooling |
| `uv.lock` | Generated and tracked | Reproducible Python dependency lockfile |

Never edit `_managed/` directly. It is disposable output owned by `skillctl`;
the next successful synchronization replaces it wholesale. Change `deps.yaml`,
`deps.local.yaml`, or the upstream repository instead.

## Synchronize Dependencies

Preview a synchronization without changing the live managed tree:

```sh
uv run --locked skillctl sync --dry-run
```

Apply it after reviewing the reported additions, updates, and removals:

```sh
uv run --locked skillctl sync
```

`skillctl` validates the complete manifest before acquisition. It rejects
unsafe paths, duplicate dependency IDs, duplicate destinations, collisions
with authored skills, illegal selection options, and sparse-checkout mappings
that cannot contain their selected sources.

Acquisition and validation happen in a staging directory. A successful run
atomically replaces `_managed/`; a failed run leaves the previous live tree in
place. The ignored `.skillctl.lock` serializes runs, and a concurrent run fails
immediately rather than interleaving changes.

## Add or Edit an Authored Skill

Authored skills live at the repository root and use the Agent Skills layout:

```text
<skill-name>/
  SKILL.md
  references/    # optional supporting documentation
  scripts/       # optional reusable tools
  templates/     # optional reusable artifacts
```

Every `SKILL.md` requires YAML frontmatter:

```yaml
---
name: skill-name
description: Use when the agent encounters the concrete situations that should load this skill.
---
```

Use a lowercase, hyphen-separated name. Keep the description focused on when
the skill applies so runtimes can make a reliable discovery decision. Do not
put project-specific instructions in a broadly reusable skill.

Root-level Markdown files are also visible to runtimes that support flat
skills. They therefore need valid `name` and `description` frontmatter too;
this README follows that contract so Reasonix can index it without warnings.

After editing an authored skill, add or update focused regression coverage when
its behavior is machine-checkable, then run the complete validation suite.

## Add a Public Managed Dependency

Public dependencies belong in `deps.yaml`. Each entry requires a stable ID, an
HTTPS GitHub URL, and exactly one selection mode. An optional `ref` pins a tag,
branch, or commit; optional `sparse` paths limit checkout size.

Use `explicit` when the exact upstream-to-local mappings are known:

```yaml
- id: example-tools
  url: https://github.com/example/agent-skills.git
  ref: v1.2.3
  sparse: [skills]
  selection:
    mode: explicit
    include:
      - source: skills/reviewing
        destination: reviewing
```

Use `discovery` when every immediate child directory containing `SKILL.md`
beneath one upstream root should be installed:

```yaml
- id: example-collection
  url: https://github.com/example/agent-skills.git
  sparse: [skills]
  selection:
    mode: discovery
    root: skills
    exclude:
      - experimental-skill
```

Prefer explicit selection when only a reviewed subset is wanted. Before
committing a dependency change, run a dry run, inspect the resolved tree, apply
the synchronization, and run the tests.

## Machine-Local Dependencies

Use the ignored `deps.local.yaml` for dependencies that should exist only on
one machine. Back this file up separately because it is intentionally not
published with the repository.

The overlay supports three operations:

- `add` introduces a complete dependency using HTTPS, SSH, or SCP-style Git
  transport;
- `extend` adds mappings to an existing `explicit` dependency; and
- `remove` excludes a dependency declared by ID in `deps.yaml`.

```yaml
version: 1
add:
  - id: local-example
    url: ssh://source-control@example.test/team/local-example.git
    selection:
      mode: explicit
      include:
        - source: skills/local-example
          destination: local-example
extend:
  - id: terraform-skill
    include:
      - source: skills/another-public-skill
        destination: another-public-skill
remove:
  - googleworkspace-cli
```

The overlay cannot redefine a base dependency, change a selection mode, extend
a discovery dependency, remove a locally added dependency, or select a source
outside an existing sparse checkout.

## Validate Changes

Use the committed lockfile for all local checks:

```sh
uv sync --locked
uv run --locked ruff check .
uv run --locked ruff format --check .
uv run --locked python -m unittest discover -s tests -v
uv run --locked skillctl sync --dry-run
```

To apply formatting after editing Python:

```sh
uv run --locked ruff format .
```

The publication tests ensure that only reviewed public source is tracked.
Generated dependencies, machine-local manifests, lock files, caches, private
credentials, and legacy updater artifacts must remain untracked.

## Troubleshooting

### Reasonix reports a missing skill description

Find the reported Markdown file and add valid YAML frontmatter containing both
`name` and `description`. Reasonix loads root-level Markdown as flat skills;
plain documentation without frontmatter produces an index warning.

```sh
reasonix doctor --json
```

### `skillctl` says another synchronization is running

Check for a live `skillctl sync` process. The `.skillctl.lock` file may remain
on disk between runs and is harmless; the operating-system lock, not the file's
mere presence, determines whether synchronization is active.

### A managed edit disappeared

Move the durable change to the authored skill, the appropriate dependency
manifest, or the upstream dependency. `_managed/` is rebuilt output and cannot
own lasting changes.

### Synchronization failed

Read the first manifest, Git, or validation error. Correct that source problem
and rerun `skillctl sync --dry-run`. The existing `_managed/` tree remains the
active version until a complete synchronization succeeds.
