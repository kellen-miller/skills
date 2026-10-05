---
name: dispatch-tickets
description: >-
  Run an approved implementation task graph through a deterministic local
  script, using fresh CLI workers in isolated Git worktrees. Use for repeated
  ticket distribution, parallel execution, and evidence-backed resume across
  coding-agent providers.
---

# Dispatch Tickets

Use the script for scheduling, process lifecycle, validation, local commits,
serial integration, and durable state. Agents own design, bounded coding,
failure diagnosis, independent review, and acceptance. Scheduling requires no
model calls. Each implementation ticket gets a fresh CLI process and worktree.

## Prepare The Approved Graph

Read [references/graph.md](references/graph.md) for the executable schema,
commands, worker contract, and recovery behavior. Derive the graph from the
approved plan; do not invent scope or publish tracker issues to make it runnable.
Tasks need settled contracts, explicit owned paths, prerequisites, and meaningful
checks. Split oversized tasks before execution. Record approval provenance in
the parent decision record; `approved: true` is a machine gate, not user consent.

Use the current isolated integration worktree and a clean, committed baseline.
Keep graph, briefs, state, and logs under ignored `.agent/work/<slug>/`.
Worker worktrees live in a sibling directory outside the checkout; each has
its own ignored task packet. Follow repository Git rules; use explicit branch/base refs.
Do not overwrite unrelated changes or silently change an existing branch.

Configure `worker_command` as an argv array, with an optional per-ticket override.
The runner must accept its task on stdin, work in its current directory, and exit
when the assignment is finished. Codex, Claude, or another CLI/wrapper can use
this protocol. Resolve the executable, model, permissions, and authentication
before running; inherit the user's configured access, and never silently switch
providers or relax sandboxing. Do not put credentials in command arguments.

## Preview And Execute

Resolve the script relative to this skill:

```sh
python3 scripts/dispatch.py --repo <integration-worktree> \
  --graph <work-item>/tickets.json status
python3 scripts/dispatch.py --repo <integration-worktree> \
  --graph <work-item>/tickets.json run --jobs 3
```

`status` validates and previews without mutations or model calls. `run` starts
local coding-agent processes and performs Git writes. A request to execute the
approved implementation covers those local actions; preview-only requests do
not. The script does not push, publish a PR, write to issue trackers, deploy,
or conduct the final independent review.

Monitor the foreground process through the runtime's resumable execution tools;
keep user updates concise. The script launches only tickets whose prerequisites
have passed integrated checks, fills available capacity, and serializes tickets
with overlapping owned paths. It integrates completed results one at a time and
runs the graph's integration checks before releasing dependent tasks.

Workers receive only their own brief plus workspace/ownership constraints.
They edit and may validate their assignment, but the script owns commits and
merges. Retain the resulting logs, exact starting commits, worker commands,
result commits, merge commits, and check outcomes as execution evidence. A zero
worker exit code alone does not establish acceptance.

## Recover And Accept

On failure the loop stops and terminates active worker groups, retaining work,
logs, and attempt history. Inspect `status`, logs, branches, and live processes.
No automatic retry, destructive reset, or worktree cleanup occurs. Retry only
selected tickets after diagnosing the failure; never retry an integrated ticket
as if it were unfinished. Use a new graph for new repair scope or changed intent.
Graph and brief changes invalidate an existing run rather than silently moving
its goalposts.

Reconcile an externally committed, inspected repair with `--accept-head <commit>`
when needed. After a merge whose integration checks failed, `--retry <id>` reruns
those checks instead of reapplying the worker result. Resolve an interrupted Git
operation before restarting; the script only recovers a completed merge when
its recorded parents match exactly. Do not edit state to bypass a failure.

The main agent verifies the combined result and planned acceptance, performs the
selected independent review, and directs bounded repairs. Script completion
means graph checks passed, not that review or publication is complete. Use
`$explain-implementation` and `$pr` for the authorized visual PR handoff.
