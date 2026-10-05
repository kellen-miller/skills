#!/usr/bin/env python3
"""Run an approved ticket graph through fresh, isolated CLI workers."""

import argparse
import copy
import fcntl
import hashlib
import json
import math
import os
import re
import signal
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path


def git(repo, *args):
    return subprocess.check_output(
        ["git", "-C", str(repo), *args], stderr=subprocess.PIPE
    )


def commit(repo, ref):
    return git(repo, "rev-parse", "--verify", f"{ref}^{{commit}}").decode().strip()


def ancestor(repo, earlier, later):
    result = subprocess.run(
        ["git", "-C", str(repo), "merge-base", "--is-ancestor", earlier, later],
        capture_output=True,
    )
    if result.returncode not in (0, 1):
        raise ValueError("cannot verify commit ancestry")

    return result.returncode == 0


def ignored_file(repo, path):
    path = path.resolve()
    relative = path.relative_to(repo)
    if relative.parts[:2] != (".agent", "work") or not path.is_file():
        raise ValueError("graph and briefs must be files under .agent/work/")

    git(repo, "check-ignore", "-q", "--", str(path))
    return path


def owned_path(value):
    if not isinstance(value, str) or not value:
        raise ValueError("owned paths must be nonempty repository-relative paths")

    parts = value.rstrip("/").split("/")
    if any(part in ("", ".", "..", ".git", ".agent") for part in parts):
        raise ValueError(f"unsafe owned path: {value}")

    if any(character in value for character in "\\*?[]\n\r\0"):
        raise ValueError(f"owned paths do not support globs: {value}")

    return "/".join(parts)


def overlaps(left, right):
    return any(
        a == b or a.startswith(b + "/") or b.startswith(a + "/")
        for a in left
        for b in right
    )


def command_array(value):
    if (
        not isinstance(value, list)
        or not value
        or not all(isinstance(arg, str) and arg and "\0" not in arg for arg in value)
    ):
        raise ValueError("commands must be nonempty argv arrays")

    return value


def load_graph(repo, path):
    path = ignored_file(repo, path)
    graph = json.loads(path.read_text())
    if not isinstance(graph, dict):
        raise ValueError("graph must be an object")

    if graph.get("version") != 1 or graph.get("approved") is not True:
        raise ValueError("graph requires version=1 and approved=true")

    base_ref = graph.get("base_ref", "")
    branch = graph.get("integration_branch", "")
    if not base_ref.startswith("refs/") or not branch.startswith("refs/heads/"):
        raise ValueError("base_ref and integration_branch require explicit Git refs")

    base = commit(repo, base_ref)
    git(repo, "check-ref-format", branch)
    if git(repo, "symbolic-ref", "HEAD").decode().strip() != branch:
        raise ValueError("checkout is not on the graph's integration branch")

    if not ancestor(repo, base, commit(repo, "HEAD")):
        raise ValueError("integration HEAD does not descend from base_ref")

    worker = command_array(graph.get("worker_command"))
    checks = graph.get("integration_checks")
    if not isinstance(checks, list) or not checks:
        raise ValueError("graph requires integration_checks")

    for check in checks:
        command_array(check)

    tickets = graph.get("tickets")
    if not isinstance(tickets, list) or not tickets:
        raise ValueError("graph requires nonempty tickets")

    tasks = {}
    inputs = []
    for ticket in tickets:
        if not isinstance(ticket, dict):
            raise ValueError("tickets must be objects")

        task = copy.deepcopy(ticket)
        task_id = task.get("id", "")
        if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,31}", task_id) or task_id in tasks:
            raise ValueError(
                "ticket IDs must be unique lowercase slugs, at most 32 chars"
            )

        if (
            not isinstance(task.get("deliverable"), str)
            or not task["deliverable"].strip()
        ):
            raise ValueError(f"{task_id} requires a deliverable")

        dependencies = task.get("depends_on")
        paths = task.get("owned_paths")
        if not isinstance(dependencies, list) or not all(
            isinstance(item, str) for item in dependencies
        ):
            raise ValueError(f"{task_id} requires depends_on as an ID list")

        if len(dependencies) != len(set(dependencies)):
            raise ValueError(f"{task_id} has duplicate dependencies")

        if not isinstance(paths, list) or not paths:
            raise ValueError(f"{task_id} requires owned_paths")

        task["owned_paths"] = [owned_path(item) for item in paths]
        message = task.get("commit_message")
        if (
            not isinstance(message, str)
            or len(message) > 50
            or not re.fullmatch(
                r"(?:feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(?:\([a-z0-9-]+\))?: [^\r\n]+",
                message,
            )
        ):
            raise ValueError(
                f"{task_id} requires a Conventional Commit subject of at most 50 chars"
            )

        task["worker_command"] = command_array(task.get("worker_command", worker))
        task_checks = task.get("checks")
        if not isinstance(task_checks, list) or not task_checks:
            raise ValueError(f"{task_id} requires checks")

        for check in task_checks:
            command_array(check)

        brief = task.get("brief")
        if not isinstance(brief, str):
            raise ValueError(f"{task_id} requires a brief path")

        brief_path = ignored_file(repo, path.parent / brief)
        content = brief_path.read_text()
        if not content.strip():
            raise ValueError(f"{task_id} brief is empty")

        task["brief_text"] = content
        inputs.append((task_id, content))
        tasks[task_id] = task

    if any(dep not in tasks for task in tasks.values() for dep in task["depends_on"]):
        raise ValueError("graph references an unknown dependency")

    remaining = set(tasks)
    while remaining:
        ready = {
            task_id
            for task_id in remaining
            if all(dep not in remaining for dep in tasks[task_id]["depends_on"])
        }
        if not ready:
            raise ValueError("graph has a dependency cycle")

        remaining -= ready

    digest = hashlib.sha256(
        json.dumps([graph, inputs], sort_keys=True).encode()
    ).hexdigest()
    return path, graph, tasks, digest


def save_state(path, state):
    with tempfile.NamedTemporaryFile("w", dir=path.parent, delete=False) as output:
        temporary = Path(output.name)
        json.dump(state, output, indent=2, sort_keys=True)
        output.write("\n")
        output.flush()
        os.fsync(output.fileno())

    os.replace(temporary, path)


def load_state(path, tasks, digest, head):
    if not path.exists():
        return {
            "version": 1,
            "graph_digest": digest,
            "integration_head": head,
            "tasks": {
                task_id: {"status": "pending", "history": []} for task_id in tasks
            },
            "adopted_heads": [],
        }

    state = json.loads(path.read_text())
    if state.get("graph_digest") != digest:
        raise ValueError("graph or briefs changed; use a new graph for revised work")

    if state.get("version") != 1 or set(state.get("tasks", {})) != set(tasks):
        raise ValueError("state does not match graph")

    if any(
        item.get("status")
        not in {
            "pending",
            "running",
            "validated",
            "merging",
            "checking",
            "integrated",
            "blocked",
        }
        for item in state["tasks"].values()
    ):
        raise ValueError("invalid task status")

    return state


def frontier(tasks, state):
    return [
        task_id
        for task_id in sorted(tasks)
        if state["tasks"][task_id]["status"] == "pending"
        and all(
            state["tasks"][dep]["status"] == "integrated"
            for dep in tasks[task_id]["depends_on"]
        )
    ]


def process_alive(pid):
    if not pid:
        return False

    try:
        os.killpg(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        # Some kernels refuse signals when the only group members are zombies.
        pass

    rows = subprocess.check_output(["ps", "-axo", "pgid=,stat="], text=True)
    for row in rows.splitlines():
        fields = row.split()
        if len(fields) == 2 and fields[0] == str(pid) and not fields[1].startswith("Z"):
            return True

    return False


def stop_process(process):
    # The group includes CLI children, not just the immediate subprocess.
    if not process_alive(process.pid):
        process.wait()
        return

    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    except PermissionError:
        if process_alive(process.pid):
            raise

        process.wait()
        return

    try:
        process.wait(timeout=3)
    except subprocess.TimeoutExpired:
        pass

    if process_alive(process.pid):
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        except PermissionError:
            if process_alive(process.pid):
                raise

    process.wait()


def expire_worker(process, expired):
    if process.poll() is None:
        expired.set()
        stop_process(process)


def settle_children(process):
    deadline = time.monotonic() + 2
    while process_alive(process.pid) and time.monotonic() < deadline:
        time.sleep(0.05)

    if process_alive(process.pid):
        stop_process(process)
        return False

    return True


def verify_integration(repo, graph, state):
    if (
        git(repo, "symbolic-ref", "HEAD").decode().strip()
        != graph["integration_branch"]
    ):
        raise ValueError("integration branch changed outside the dispatcher")

    if commit(repo, "HEAD") != state["integration_head"]:
        raise ValueError("integration HEAD changed outside the dispatcher")

    if git(repo, "status", "--porcelain", "--untracked-files=all"):
        raise ValueError(
            "integration checkout changed; preserve and reconcile local edits"
        )


def changed_paths(worker):
    tracked = git(worker, "diff", "HEAD", "--name-only", "--no-renames", "-z")
    untracked = git(worker, "ls-files", "--others", "--exclude-standard", "-z")
    return sorted({path for path in (tracked + untracked).decode().split("\0") if path})


def verify_worker(repo, task, item):
    worker = Path(item["worker_path"])
    if git(worker, "symbolic-ref", "HEAD").decode().strip() != item["worker_branch"]:
        raise ValueError("worker branch changed")

    if commit(worker, "HEAD") != item["starting_commit"]:
        raise ValueError("worker committed or changed HEAD; script owns Git transfer")

    paths = changed_paths(worker)
    for path in paths:
        if not any(
            path == owned or path.startswith(owned + "/")
            for owned in task["owned_paths"]
        ):
            raise ValueError(f"worker changes unowned path: {path}")

    if commit(repo, "HEAD") != item["expected_integration_head"]:
        raise ValueError("integration HEAD changed outside the dispatcher")

    return worker, paths


def run_checks(commands, cwd, log_path, timeout):
    results = []
    with log_path.open("ab") as log:
        for argv in commands:
            log.write(("\nCHECK " + json.dumps(argv) + "\n").encode())
            log.flush()
            process = subprocess.Popen(
                argv, cwd=cwd, stdout=log, stderr=log, start_new_session=True
            )
            try:
                code = process.wait(timeout=timeout)
            except (subprocess.TimeoutExpired, KeyboardInterrupt):
                stop_process(process)
                raise

            settled = settle_children(process)
            results.append({"command": argv, "exit_code": code, "log": str(log_path)})
            if code:
                raise ValueError(f"check failed ({code}); see {log_path}")

            if not settled:
                raise ValueError(f"check left live child processes; see {log_path}")

    return results


def integrate(repo, graph, task_id, item, state, state_path, timeout):
    if item["status"] == "validated":
        verify_integration(repo, graph, state)
        item["status"] = "merging"
        item["merge_base"] = state["integration_head"]
        save_state(state_path, state)
        try:
            git(repo, "merge", "--no-ff", "--no-edit", item["result_commit"])
        except subprocess.CalledProcessError:
            if Path(
                git(
                    repo,
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-path",
                    "MERGE_HEAD",
                )
                .decode()
                .strip()
            ).exists():
                git(repo, "merge", "--abort")

            raise ValueError(
                f"merge failed for {task_id}; worker result retained"
            ) from None

    if item["status"] == "merging":
        head = commit(repo, "HEAD")
        parents = (
            git(repo, "rev-list", "--parents", "-n", "1", head).decode().split()[1:]
        )
        if parents != [item["merge_base"], item["result_commit"]]:
            raise ValueError(
                "cannot reconcile interrupted merge; inspect Git before resuming"
            )

        item.update(status="checking", integration_commit=head)
        state["integration_head"] = head
        save_state(state_path, state)

    if item["status"] == "checking":
        verify_integration(repo, graph, state)
        proof = run_checks(
            graph["integration_checks"],
            repo,
            state_path.parent / "integration.log",
            timeout,
        )
        verify_integration(repo, graph, state)
        item.update(status="integrated", integration_checks=proof)
        save_state(state_path, state)
        print(
            json.dumps(
                {
                    "event": "integrated",
                    "task": task_id,
                    "commit": state["integration_head"],
                }
            ),
            flush=True,
        )


def run(args, repo, graph_path, graph, tasks, digest, directory):
    state_path = directory / "state.json"
    state = load_state(state_path, tasks, digest, commit(repo, "HEAD"))
    active = {}
    current = None
    try:
        # Reconcile only a merge that this script recorded before a crash.
        for task_id, item in state["tasks"].items():
            if item["status"] == "merging":
                current = task_id
                if commit(repo, "HEAD") == item["merge_base"]:
                    item["status"] = "validated"
                else:
                    integrate(
                        repo,
                        graph,
                        task_id,
                        item,
                        state,
                        state_path,
                        args.check_timeout,
                    )

        for item in state["tasks"].values():
            if item["status"] in {"running", "blocked"} and process_alive(
                item.get("pid")
            ):
                raise ValueError(
                    "a previous worker group is still live; stop it before resuming"
                )

        head = commit(repo, "HEAD")
        if head != state["integration_head"]:
            if (
                not args.accept_head
                or commit(repo, args.accept_head) != head
                or not ancestor(repo, state["integration_head"], head)
            ):
                raise ValueError(
                    "integration HEAD drifted; inspect and explicitly --accept-head <commit>"
                )

            state["adopted_heads"].append(
                {
                    "previous": state["integration_head"],
                    "accepted": head,
                    "status": "checking",
                }
            )
            state["integration_head"] = head

        for task_id in args.retry:
            item = state["tasks"].get(task_id)
            if not item or item["status"] not in {"blocked", "running"}:
                raise ValueError(
                    "--retry requires a blocked or interrupted running ticket"
                )

            item["history"].append(
                {key: value for key, value in item.items() if key != "history"}
            )
            if item.get("integration_commit"):
                item["status"] = "checking"
            else:
                history = item["history"]
                item.clear()
                item.update(status="pending", history=history)

        if any(
            item["status"] in {"blocked", "running"} for item in state["tasks"].values()
        ):
            raise ValueError(
                "blocked or interrupted tickets require inspection and explicit --retry <id>"
            )

        save_state(state_path, state)
        for adoption in state["adopted_heads"]:
            if adoption["status"] == "checking":
                proof = run_checks(
                    graph["integration_checks"],
                    repo,
                    directory / "integration.log",
                    args.check_timeout,
                )
                if git(repo, "status", "--porcelain", "--untracked-files=all"):
                    raise ValueError("integration checks left uncommitted changes")

                adoption.update(status="verified", checks=proof)
                save_state(state_path, state)

        for task_id, item in state["tasks"].items():
            if item["status"] in {"validated", "checking"}:
                current = task_id
                integrate(
                    repo, graph, task_id, item, state, state_path, args.check_timeout
                )

        while any(item["status"] != "integrated" for item in state["tasks"].values()):
            # Observe failures before opening another provider session.
            for task_id, (process, _, _, expired) in active.items():
                if expired.is_set():
                    current = task_id
                    raise ValueError(f"worker timed out: {task_id}")

                code = process.poll()
                if code is not None and code != 0:
                    current = task_id
                    raise ValueError(
                        f"worker failed ({code}); see {state['tasks'][task_id]['worker_log']}"
                    )

            for task_id in frontier(tasks, state):
                if len(active) >= args.jobs:
                    break

                task = tasks[task_id]
                if any(
                    overlaps(task["owned_paths"], tasks[other]["owned_paths"])
                    for other in active
                ):
                    continue

                current = task_id
                verify_integration(repo, graph, state)
                item = state["tasks"][task_id]
                attempt = len(item["history"]) + 1
                worker_root = repo.parent / (
                    f".{repo.name}-dispatch-"
                    + hashlib.sha256(str(graph_path).encode()).hexdigest()[:12]
                )
                worker = worker_root / f"{task_id}-{attempt}"
                branch = (
                    graph["integration_branch"].removeprefix("refs/heads/")
                    + f"-{task_id}-{attempt}"
                )
                head = commit(repo, "HEAD")
                item.update(
                    status="running",
                    starting_commit=head,
                    worker_path=str(worker),
                    worker_branch=f"refs/heads/{branch}",
                    pid=None,
                )
                save_state(state_path, state)
                git(repo, "worktree", "add", "-b", branch, str(worker), head)
                packet = worker / ".agent" / "work" / graph_path.parent.name / "task.md"
                git(worker, "check-ignore", "-q", "--", str(packet))
                packet.parent.mkdir(parents=True, exist_ok=True)
                prompt = (
                    f"Implement only ticket {task_id}: {task['deliverable']}\n"
                    f"Worktree: {worker}\nStarting commit: {head}\n"
                    f"Owned paths: {json.dumps(task['owned_paths'])}\n"
                    "Read actual source and repository instructions. Keep all edits in this worktree.\n"
                    "Do not commit, switch branches, integrate other work, publish, or spawn another orchestration team.\n"
                    "The dispatcher owns commits, checks, and integration. Report remaining risks.\n\n"
                    + task["brief_text"]
                )
                packet.write_text(prompt)
                log_path = directory / f"{task_id}-{attempt}.log"
                log = log_path.open("wb")
                # A file supplies stdin without a pipe that can block on a large brief.
                with packet.open("rb") as incoming:
                    try:
                        process = subprocess.Popen(
                            task["worker_command"],
                            cwd=worker,
                            stdin=incoming,
                            stdout=log,
                            stderr=log,
                            start_new_session=True,
                        )
                    except BaseException:
                        log.close()
                        raise

                expired = threading.Event()
                timer = threading.Timer(
                    args.worker_timeout, expire_worker, args=(process, expired)
                )
                timer.daemon = True
                active[task_id] = (process, log, timer, expired)
                timer.start()
                item.update(
                    pid=process.pid,
                    worker_command=task["worker_command"],
                    worker_log=str(log_path),
                )
                save_state(state_path, state)
                print(
                    json.dumps(
                        {"event": "started", "task": task_id, "worktree": str(worker)}
                    ),
                    flush=True,
                )

            current = None
            if not active:
                raise ValueError("no runnable tickets; inspect graph and state")

            completed = [
                task_id
                for task_id in sorted(active)
                if active[task_id][0].poll() is not None
            ]
            for task_id, (_, _, _, expired) in active.items():
                if expired.is_set():
                    current = task_id
                    raise ValueError(f"worker timed out: {task_id}")

            if not completed:
                time.sleep(0.1)
                continue

            for task_id in completed:
                current = task_id
                process, log, timer, expired = active[task_id]
                timer.cancel()
                timer.join()
                log.close()
                settled = settle_children(process)
                if expired.is_set():
                    raise ValueError(f"worker timed out: {task_id}")

                item = state["tasks"][task_id]
                if process.returncode:
                    raise ValueError(
                        f"worker failed ({process.returncode}); see {item['worker_log']}"
                    )

                if not settled:
                    raise ValueError("worker left live child processes")

                active.pop(task_id)
                item["expected_integration_head"] = state["integration_head"]
                worker, paths = verify_worker(repo, tasks[task_id], item)
                proof = run_checks(
                    tasks[task_id]["checks"],
                    worker,
                    Path(item["worker_log"]),
                    args.check_timeout,
                )
                worker, paths = verify_worker(repo, tasks[task_id], item)
                if not paths:
                    raise ValueError(
                        "ticket produced no changes; inspect acceptance and task scope"
                    )

                git(worker, "add", "--", *paths)
                git(worker, "commit", "-m", tasks[task_id]["commit_message"])
                item.update(
                    status="validated",
                    result_commit=commit(worker, "HEAD"),
                    worker_checks=proof,
                )
                save_state(state_path, state)
                integrate(
                    repo, graph, task_id, item, state, state_path, args.check_timeout
                )

        print(
            json.dumps(
                {"event": "complete", "integration_head": state["integration_head"]}
            ),
            flush=True,
        )
    except BaseException as error:
        if current and state["tasks"][current]["status"] != "integrated":
            item = state["tasks"][current]
            completed_merge = item["status"] == "merging" and git(
                repo, "rev-list", "--parents", "-n", "1", "HEAD"
            ).decode().split()[1:] == [item["merge_base"], item["result_commit"]]
            if not completed_merge:
                item["status"] = "blocked"

            item["reason"] = str(error)

        for task_id, (process, log, timer, _) in active.items():
            timer.cancel()
            timer.join()
            stop_process(process)
            log.close()
            state["tasks"][task_id]["status"] = "blocked"
            if task_id != current:
                state["tasks"][task_id]["reason"] = (
                    "run stopped; inspect retained work before retry"
                )

        save_state(state_path, state)
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--graph", type=Path, required=True)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("status")
    execute = commands.add_parser("run")
    execute.add_argument("--jobs", type=int, required=True)
    execute.add_argument("--worker-timeout", type=float, default=1800)
    execute.add_argument("--check-timeout", type=float, default=300)
    execute.add_argument(
        "--retry",
        action="append",
        default=[],
        help="explicitly retry an inspected failed ticket",
    )
    execute.add_argument(
        "--accept-head", help="adopt an inspected external repair commit"
    )
    args = parser.parse_args()
    try:
        repo = args.repo.resolve()
        if (
            Path(git(repo, "rev-parse", "--show-toplevel").decode().strip()).resolve()
            != repo
        ):
            raise ValueError("--repo must name the integration worktree root")

        graph_path, graph, tasks, digest = load_graph(repo, args.graph)
        directory = graph_path.parent / "dispatch"
        if args.command == "status":
            head = commit(repo, "HEAD")
            state = load_state(directory / "state.json", tasks, digest, head)
            print(
                json.dumps(
                    {**state, "ready": frontier(tasks, state), "observed_head": head},
                    indent=2,
                )
            )
        else:
            if args.jobs < 1 or any(
                not math.isfinite(value) or value <= 0
                for value in (args.worker_timeout, args.check_timeout)
            ):
                raise ValueError("jobs and timeouts must be positive")

            if git(repo, "status", "--porcelain", "--untracked-files=all"):
                raise ValueError("integration worktree must be clean before run")

            directory.mkdir(exist_ok=True)
            with (repo / ".agent/work/.dispatch.lock").open("a+") as lock:
                try:
                    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                except BlockingIOError as error:
                    raise ValueError("dispatcher is already running") from error

                run(args, repo, graph_path, graph, tasks, digest, directory)

        return 0
    except (
        ValueError,
        KeyError,
        TypeError,
        OSError,
        subprocess.SubprocessError,
        KeyboardInterrupt,
    ) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    # Foreground cancellation must also stop CLI children before returning.
    def interrupt(signum, frame):
        raise KeyboardInterrupt(f"received signal {signum}")

    signal.signal(signal.SIGTERM, interrupt)
    raise SystemExit(main())
