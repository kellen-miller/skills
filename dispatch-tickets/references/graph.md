# Ticket graph and CLI protocol

Python 3.11+, Git, `ps`, and POSIX process groups/file locks are required. No provider
SDK is required. This script is intended for macOS/Linux. Its checks and owned
paths validate results; they are not an operating-system security boundary.
Configure each worker CLI's own permissions for the repository.

## Graph

Save `tickets.json` and briefs under Git-ignored `.agent/work/<slug>/`:

```json
{
  "version": 1,
  "approved": true,
  "base_ref": "refs/remotes/origin/main",
  "integration_branch": "refs/heads/feat/example",
  "worker_command": ["codex", "exec", "--sandbox", "workspace-write", "-"],
  "integration_checks": [["uv", "run", "--locked", "python", "-m", "unittest", "discover", "-s", "tests"]],
  "tickets": [
    {
      "id": "export",
      "deliverable": "Export the selected records as JSON",
      "commit_message": "feat: add JSON export",
      "brief": "tasks/export.md",
      "depends_on": [],
      "owned_paths": ["src/export", "tests/test_export.py"],
      "checks": [["uv", "run", "--locked", "python", "-m", "unittest", "tests.test_export"]]
    },
    {
      "id": "cli",
      "deliverable": "Expose the verified exporter in the CLI",
      "commit_message": "feat: expose export in the CLI",
      "brief": "tasks/cli.md",
      "depends_on": ["export"],
      "owned_paths": ["src/cli.py", "tests/test_cli.py"],
      "worker_command": ["claude", "-p", "--permission-mode", "acceptEdits"],
      "checks": [["uv", "run", "--locked", "python", "-m", "unittest", "tests.test_cli"]]
    }
  ]
}
```

Adapt commands to the target project. Both CLI examples use configured auth and
model defaults; set explicit model/profile options when desired. Configure
noninteractive command permissions using the selected CLI's supported settings.
Per-ticket `worker_command` replaces the default array entirely, allowing a
provider or model appropriate to each task. A wrapper can adapt any CLI that
does not directly accept stdin. Commands run without a shell; shell features
require an explicitly chosen shell in the array.

`base_ref` must be an explicit Git ref whose commit is an ancestor of integration
HEAD. The script records each worker's exact integration starting commit,
including integrated prerequisites, rather than starting from remote main.
`integration_branch` is the full local branch ref and must be currently checked
out. Ticket IDs are unique lowercase slugs, at most 32 characters. Dependencies
must exist and be acyclic. Owned paths are exact files or directory prefixes,
relative to the repo; no globs, traversal, `.git`, or `.agent` ownership.
`commit_message` is the repository-appropriate Conventional Commit subject,
at most 50 characters. Breaking changes need a separate approved integration
commit with the required footer; do not hide them in generated ticket subjects.

Each brief explains approved decisions, dependency contracts, observable
acceptance, useful source/research pointers, and boundaries. Keep it scoped to
one context. Briefs are resolved relative to the graph and must be ignored files
under `.agent/work/`. The graph and brief contents are fingerprinted; revised
scope requires a new work item/graph containing only the remaining approved work.

## Execution And Evidence

`status` reads existing state or reports initial readiness. `run --jobs N` holds
an exclusive lock, orders ready IDs lexically, launches at most N workers, and
avoids overlapping owned paths. Completion order can vary; dispatch decisions
are deterministic for the observed completion events. Defaults: 1800 seconds
per worker and 300 seconds per check, configurable with `--worker-timeout` and
`--check-timeout`.

Each worker runs in a new branch and worktree under a uniquely named sibling
directory outside the checkout. This keeps recursive integration checks from
walking unfinished worker copies. Absolute worker paths are recorded in state.
Its task packet is copied into its own ignored `.agent/work/` directory and
supplied on stdin. Stdout/stderr go to an attempt log. Workers must leave HEAD
unchanged and edit only owned paths. Successful worker execution is followed
by task checks, scope/branch/HEAD verification, a local Conventional Commit,
a serial merge, and integration checks. Configure checks as local, repeatable
commands without publication/deployment side effects.

State is saved atomically under `dispatch/state.json`, with logs alongside it.
The lifecycle is pending → running → validated → merging → checking → integrated;
failures become blocked. Commands, exit codes, exact commits, paths, and prior
attempts are retained. The script stops the run on a failure and preserves the
integration branch, including any merge whose checks failed. Nothing is pushed
or cleaned up automatically. Keep `.agent/work/` ignored in worker worktrees too.

## Resume

Run the same command to resume a successfully interrupted integration or skip
already integrated tickets. Running/blocked tickets require explicit inspection
and `run --jobs N --retry <id>`; repeat the flag for several diagnosed failures.
A live previous worker process group prevents retry. Before retrying an attempt
interrupted between process launch and PID recording, verify no worker remains
running. New attempts use a new branch/worktree at the current integration tip,
retaining prior work rather than resetting it. Port useful partial work into a
new attempt deliberately, after checking it against current source.

If integration checks failed after the ticket merged, retry repeats only those
checks. If a human/main agent committed an inspected repair outside the loop,
add `--accept-head <exact-commit>` to adopt that descendant integration HEAD;
this is an explicit reconciliation, recorded in state. It does not approve
changed scope. Integrated tickets cannot be retried; new implementation repairs
belong in a new graph at the updated tip.

An interrupted merge is recovered only when HEAD has the two recorded merge
parents in the expected order, or no merge occurred and HEAD is unchanged.
In-progress conflicts or unexplained Git changes require manual reconciliation.
Do not run two graphs concurrently on one integration checkout. Worker CLIs
must stay within their worktrees and not leave background processes behind.
