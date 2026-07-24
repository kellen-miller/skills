# Agent skills

This repository owns the authored and managed skills installed at
`~/.agents/skills`. Set it up explicitly:

    git clone https://github.com/kellen-miller/skills.git \
      ~/development/github/kellen-miller/skills
    ln -s ~/development/github/kellen-miller/skills ~/.agents/skills
    python3 -m venv ~/.agents/skills/.venv
    ~/.agents/skills/.venv/bin/python -m pip install \
      -r ~/.agents/skills/requirements.txt
    ~/.agents/skills/.venv/bin/python ~/.agents/skills/update.py sync

The skill directories at the repository root are authored content. Public
upstream dependencies are declared in `deps.yaml`; `update.py sync` resolves
them into ignored `_managed/`. That entire directory is disposable and owned
by the updater. The updater stages every dependency before atomically replacing
the live tree, so acquisition or validation failure leaves the previous tree
unchanged. Use `python update.py sync --dry-run` to resolve and report changes
without installing them.

Each dependency chooses exactly one selection mode. `explicit` lists exact
source and destination mappings. `discovery` selects immediate child
directories containing `SKILL.md` beneath one root, with an optional exclude
list.

## Machine-local dependencies

Create an ignored `deps.local.yaml` for dependencies that belong only on one
machine. The overlay is additive: `add` introduces a complete new dependency,
while `extend` adds mappings to an existing explicit dependency.

    version: 1
    add:
      - id: local-example
        url: https://example.test/team/local-example.git
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

The overlay cannot remove or redefine base dependencies, change selection
modes, or extend discovery dependencies. Keep it backed up separately; it is
not part of this repository.
