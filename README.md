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

`grill-plan-build` coordinates design, investigation, and task decomposition.
`dispatch-tickets` runs an approved graph through configurable CLI workers in
isolated worktrees, retaining validation and recovery evidence.
`explain-implementation` provides the visual explanation in the PR itself.

## Requirements

- macOS or Linux (uses POSIX file locks)
- Git
- Rust and Cargo (minimum version in `Cargo.toml`; the committed toolchain selects stable)
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
cargo install --path . --locked
skillctl sync
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
| `src/` | Authored | Rust skill synchronization, dispatcher, and command entry points |
| `templates/` | Authored | Embedded lock JSON and router/index Markdown templates |
| `Cargo.toml`, `Cargo.lock` | Authored/generated | Rust package and locked dependencies |
| `tests/` | Authored | Publication and authored skill regression coverage |
| `.jscpd.json` | Authored | Duplication policy for Rust source and tests |

Never edit `_managed/` directly. It is disposable output owned by `skillctl`;
the next successful synchronization repairs any local changes. Change `deps.yaml`,
`deps.local.yaml`, or the upstream repository instead.

## Synchronize Dependencies

Preview a synchronization without changing the live managed tree:

```sh
skillctl sync --dry-run
```

Apply it after reviewing the reported additions, updates, and removals:

```sh
skillctl sync
```

`skillctl` discovers the checkout from the current directory or its parents,
then falls back to `~/.agents/skills`. Pass `--root PATH` after either command to
select a checkout explicitly. Installed binaries include their output templates
and do not depend on the build directory. Ensure Cargo's installation directory
(usually `~/.cargo/bin`) is on `PATH`. The same install supplies `dispatch-tickets`.

`skillctl` validates the complete manifest before acquisition. It rejects
unsafe paths, duplicate dependency IDs, duplicate destinations, collisions
with authored skills, illegal selection options, and sparse-checkout mappings
that cannot contain their selected sources.

Each managed skill's `SKILL.md` frontmatter `name` is rewritten to its
destination directory name. Upstream collections often reuse generic names
such as `testing` or `review`; destinations are unique, so the rewritten names
are too, and runtimes that index by `name` see no collisions.

`skillctl` keeps disposable Git checkouts in `~/.cache/skillctl/`, or
`$XDG_CACHE_HOME/skillctl/` when `XDG_CACHE_HOME` is an absolute path. It refreshes
up to four dependencies concurrently, reusing downloaded Git objects. Every run
checks upstream; a failed fetch never falls back to stale cached content.

The ignored `.skillctl.lock` records each dependency's URL, requested ref, and
exact resolved commit, together with selected Git subtree IDs, dependency
settings, generator source, and a fingerprint of installed output. If the
generation inputs and live output still match, generation and replacement are
skipped.
Upstream changes outside selected paths do not require regeneration. Sources
with repository-local symlinks use the full repository tree ID because their
targets may sit outside the selected paths.

Changed inputs trigger validation and generation of a complete staging tree.
If its contents differ, the whole `_managed/` tree is replaced using the existing
rename-and-rollback procedure. Renames, removals, discovery changes, and router
changes therefore remove obsolete output. Identical output stays in place even
when inputs changed. Acquisition or validation failure leaves the live tree in
place; an installation rename failure restores it.

The same `.skillctl.lock` also serializes runs through an operating-system lock.
The lock JSON, router entry points, and description indexes are rendered from
embedded MiniJinja templates. JSON values are encoded with Serde before insertion.
Metadata is written in place while that lock is held, preserving the file's
inode and preventing concurrent runs from bypassing it. An empty, corrupt, or
interrupted metadata write triggers regeneration on the next run. A shared
cache lock also prevents different checkouts from modifying cached repositories
during staging. A concurrent run fails immediately rather than interleaving changes.

`sync` checks upstream and refreshes the recorded resolutions on every successful
run, including when changes outside selected paths leave installed output
unchanged. Use a manifest `ref` to pin a dependency. The lockfile remains local
because it includes machine-local dependencies and installation state.

Dry runs may refresh the Git cache but leave installed output and existing
lockfile contents unchanged. Between runs, deleting the cache or lockfile is
safe; the next sync downloads or regenerates what it needs. Do not delete the
lockfile during a run. Cache entries are keyed by dependency ID, URL, ref, and
sparse paths; entries for old configurations can be removed by deleting
`~/.cache/skillctl/` (or its configured equivalent).

## Install for Claude Code

Claude Code does not read `~/.agents/skills`, and it only discovers skills one
level deep, so the nested `_managed/` tree is invisible to it. `skillctl sync`
therefore finishes by copying every authored and managed skill flat into
`~/.claude/skills`. Run the copy on its own after editing an authored skill:

```sh
skillctl install-claude --dry-run
skillctl install-claude
```

Pass `--claude-target` to install somewhere else, or `sync --no-claude` to
skip the copy. The command records what it installed in
`<target>/.skillctl-owned` and only replaces or removes those entries; other
content, such as Claude's own `synced/` directory, is left alone. Unchanged skills
stay in place; changed owned entries are replaced and deselected owned entries
are removed. It fails
without changing anything if a skill name already exists in the target but is
not recorded as owned.

Set `claude: false` on a dependency to keep its skills out of Claude Code
while still materializing them in `_managed/`. Use it for skills a repository
already ships in its own `.claude/skills`, which Claude loads when working in
that repository. It requires `explicit` selection.

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
branch, or commit; optional `sparse` paths limit checkout size; optional
`claude: false` hides the dependency from Claude Code.

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

### Route Large Collections

Every installed skill's name and description sits in each agent session, so a
collection of dozens of skills crowds discovery even when it is rarely used.
Add a `router` to collapse a dependency into one skill:

```yaml
- id: example-collection
  url: https://github.com/example/agent-skills.git
  router:
    name: example-skills
    description: >-
      Use for Example work: the concrete situations any of its skills cover.
  selection:
    mode: explicit
    include:
      - source: skills/reviewing
        destination: reviewing
```

`skillctl sync` installs `_managed/example-skills/SKILL.md` and nests guides
under `_managed/example-skills/skills/<destination>/`. It renames every nested
`SKILL.md` to `GUIDE.md` and updates local Markdown references to renamed files,
so recursive discovery exposes only the router. A conflicting `GUIDE.md` fails
synchronization before replacing the live tree.

Small routers include full guide descriptions. Routers with more than 12 guides
or over 8,000 characters of index entries instead list guide names grouped into
linked indexes of at most 12 descriptions. Load the relevant index, then the
selected guide; resolve paths from the file containing them.

Write a concise router description covering its task branches: only that
frontmatter description is preloaded in the agent's skill catalog. Full guide
descriptions remain available in the indexes without inflating every turn.

Nested destinations only need to be unique within their router. A nested skill
cannot be invoked directly by name.

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

Use the committed Cargo lockfile for local checks:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features
cargo test --locked --workspace --doc
cargo build --locked --workspace --all-targets --all-features
cpd --config .jscpd.json --no-tips --no-colors
./target/debug/skillctl sync --dry-run
```

Install the Rust duplication tool with `cargo install jscpd --version 5.4.1 --locked`.
CI uses the shared `rust-check` and `code-duplication` actions pinned to the
reviewed commit from [CI PR #20](https://github.com/kellen-miller/ci/pull/20).
All maintained executable code and tests in this repository use Rust; downloaded
upstream skill content retains its original implementation languages.

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
