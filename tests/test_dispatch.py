import fcntl
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "dispatch-tickets/scripts/dispatch.py"


class DispatchTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.repo = self.root / "repo"
        self.repo.mkdir()
        self.git("init", "-b", "main")
        self.git("config", "user.name", "Test User")
        self.git("config", "user.email", "test@example.com")
        (self.repo / ".gitignore").write_text(".agent/work/\n")
        (self.repo / "README.md").write_text("initial\n")
        self.git("add", ".")
        self.git("commit", "-m", "initial")
        self.initial = self.git("rev-parse", "HEAD").strip()
        self.git("update-ref", "refs/remotes/origin/main", self.initial)
        self.work = self.repo / ".agent/work/feature"
        self.work.mkdir(parents=True)
        self.graph = self.work / "tickets.json"
        self.runner = self.root / "worker.py"
        self.runner.write_text(
            "import pathlib, signal, subprocess, sys, time\n"
            "ticket, mode = sys.argv[1:3]\n"
            "assert 'Implement only ticket' in sys.stdin.read()\n"
            "if mode == 'need-a':\n"
            "    assert pathlib.Path('a.txt').read_text() == 'a\\n'\n"
            "if mode == 'barrier':\n"
            "    folder = pathlib.Path(sys.argv[3])\n"
            "    (folder / ticket).touch()\n"
            "    for _ in range(100):\n"
            "        if (folder / 'a').exists() and (folder / 'b').exists(): break\n"
            "        time.sleep(0.02)\n"
            "    else: raise SystemExit(9)\n"
            "if mode == 'slow': time.sleep(10)\n"
            "if mode == 'child-fail':\n"
            "    child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)'])\n"
            "    pathlib.Path(sys.argv[3]).write_text(str(child.pid))\n"
            "    raise SystemExit(7)\n"
            "if mode == 'main-dirty':\n"
            "    pathlib.Path(sys.argv[3]).write_text('unrelated edit')\n"
            "if mode == 'deadline':\n"
            "    clock = pathlib.Path(sys.argv[3])\n"
            "    clock.write_text(str(time.monotonic()))\n"
            "    def stopped(signum, frame):\n"
            "        clock.write_text(clock.read_text() + '\\n' + str(time.monotonic()))\n"
            "        raise SystemExit(8)\n"
            "    signal.signal(signal.SIGTERM, stopped)\n"
            "    time.sleep(30)\n"
            "if mode == 'fail': raise SystemExit(7)\n"
            "if mode == 'outside': pathlib.Path('outside.txt').write_text('bad')\n"
            "if mode != 'empty': pathlib.Path(ticket + '.txt').write_text(ticket + '\\n')\n"
        )
        self.data = {
            "version": 1,
            "approved": True,
            "base_ref": "refs/remotes/origin/main",
            "integration_branch": "refs/heads/main",
            "worker_command": [sys.executable, str(self.runner), "a", "ok"],
            "integration_checks": [[sys.executable, "-c", "assert True"]],
            "tickets": [],
        }
        self.add_ticket("a")

    def git(self, *args):
        return subprocess.check_output(
            ["git", "-C", str(self.repo), *args], stderr=subprocess.PIPE, text=True
        )

    def add_ticket(self, ticket, dependencies=None, mode="ok", paths=None):
        (self.work / f"{ticket}.md").write_text(
            f"Deliver {ticket}. Test its behavior.\n"
        )
        self.data["tickets"].append(
            {
                "id": ticket,
                "deliverable": f"deliver {ticket}",
                "commit_message": f"feat: implement {ticket}",
                "brief": f"{ticket}.md",
                "depends_on": dependencies or [],
                "owned_paths": paths or [f"{ticket}.txt"],
                "worker_command": [sys.executable, str(self.runner), ticket, mode],
                "checks": [
                    [
                        sys.executable,
                        "-c",
                        f"from pathlib import Path; assert Path('{ticket}.txt').exists()",
                    ]
                ],
            }
        )

    def call(self, *args, write=True):
        if write:
            self.graph.write_text(json.dumps(self.data))

        return subprocess.run(
            [
                sys.executable,
                str(SCRIPT),
                "--repo",
                str(self.repo),
                "--graph",
                str(self.graph),
                *args,
            ],
            capture_output=True,
            text=True,
            timeout=15,
        )

    def state(self):
        return json.loads((self.work / "dispatch/state.json").read_text())

    def assert_passed(self, result):
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)

    def test_status_is_read_only_and_reports_dependency_frontier(self):
        self.add_ticket("b", ["a"])
        self.add_ticket("c")
        result = self.call("status")
        self.assert_passed(result)
        self.assertEqual(json.loads(result.stdout)["ready"], ["a", "c"])
        self.assertFalse((self.work / "dispatch").exists())
        self.assertEqual(self.git("status", "--porcelain"), "")

    def test_parallel_workers_then_dependent_start_from_integrated_tip(self):
        self.data["tickets"][0]["worker_command"][-1] = "barrier"
        self.data["tickets"][0]["worker_command"].append(str(self.root))
        self.add_ticket("b", mode="barrier")
        self.data["tickets"][1]["worker_command"].append(str(self.root))
        self.add_ticket("c", ["a"], mode="need-a")
        result = self.call("run", "--jobs", "2")
        self.assert_passed(result)
        state = self.state()
        for task in state["tasks"].values():
            self.assertEqual(task["status"], "integrated")
            self.assertEqual(task["worker_checks"][0]["exit_code"], 0)
            self.assertEqual(task["integration_checks"][0]["exit_code"], 0)
            self.assertTrue(Path(task["worker_log"]).exists())

        self.assertEqual(state["tasks"]["a"]["starting_commit"], self.initial)
        self.assertFalse(
            Path(state["tasks"]["a"]["worker_path"]).is_relative_to(self.repo)
        )
        self.assertEqual(state["tasks"]["b"]["starting_commit"], self.initial)
        self.assertNotEqual(state["tasks"]["c"]["starting_commit"], self.initial)
        self.assertTrue((self.repo / "a.txt").exists())
        self.assertTrue((self.repo / "b.txt").exists())
        self.assertTrue((self.repo / "c.txt").exists())
        head = self.git("rev-parse", "HEAD")
        self.assert_passed(self.call("run", "--jobs", "2"))
        self.assertEqual(self.git("rev-parse", "HEAD"), head)

    def test_overlapping_ownership_serializes_independent_tickets(self):
        self.data["tickets"][0]["owned_paths"] = ["a.txt", "b.txt"]
        self.add_ticket("b", paths=["a.txt", "b.txt"])
        self.assert_passed(self.call("run", "--jobs", "2"))
        state = self.state()["tasks"]
        self.assertEqual(
            state["b"]["starting_commit"], state["a"]["integration_commit"]
        )

    def test_rejects_cycles_unknown_dependencies_and_unsafe_paths_before_mutation(self):
        for dependencies, paths, error in [
            (["a"], ["a.txt"], "cycle"),
            (["missing"], ["a.txt"], "unknown"),
            ([], ["../outside"], "unsafe"),
        ]:
            with self.subTest(error=error):
                self.data["tickets"][0].update(
                    depends_on=dependencies, owned_paths=paths
                )
                result = self.call("run", "--jobs", "1")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(error, result.stderr)
                self.assertFalse((self.work / "dispatch").exists())
                self.assertEqual(self.git("rev-parse", "HEAD").strip(), self.initial)

    def test_unowned_changes_are_retained_and_never_integrated(self):
        self.data["tickets"][0]["worker_command"][-1] = "outside"
        result = self.call("run", "--jobs", "1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unowned path", result.stderr)
        item = self.state()["tasks"]["a"]
        self.assertEqual(item["status"], "blocked")
        self.assertTrue((Path(item["worker_path"]) / "outside.txt").exists())
        self.assertFalse((self.repo / "outside.txt").exists())
        self.assertEqual(self.git("rev-parse", "HEAD").strip(), self.initial)

    def test_failed_worker_requires_explicit_retry_and_preserves_attempt(self):
        self.data["tickets"][0]["worker_command"][-1] = "fail"
        self.assertNotEqual(self.call("run", "--jobs", "1").returncode, 0)
        first = self.state()["tasks"]["a"]["worker_path"]
        result = self.call("run", "--jobs", "1")
        self.assertIn("explicit --retry", result.stderr)
        # The command/brief stay immutable; repair the test worker's external failure.
        self.runner.write_text(
            self.runner.read_text().replace(
                "if mode == 'fail': raise SystemExit(7)", "if mode == 'fail': pass"
            )
        )
        self.assert_passed(self.call("run", "--jobs", "1", "--retry", "a"))
        item = self.state()["tasks"]["a"]
        self.assertEqual(len(item["history"]), 1)
        self.assertNotEqual(item["worker_path"], first)
        self.assertTrue(Path(first).is_dir())

    def test_worker_check_failure_does_not_commit_or_unblock_dependents(self):
        self.data["tickets"][0]["checks"] = [
            [sys.executable, "-c", "raise SystemExit(4)"]
        ]
        self.add_ticket("b", ["a"])
        result = self.call("run", "--jobs", "2")
        self.assertIn("check failed (4)", result.stderr)
        state = self.state()["tasks"]
        self.assertEqual(state["a"]["status"], "blocked")
        self.assertEqual(state["b"]["status"], "pending")
        self.assertEqual(self.git("rev-parse", "HEAD").strip(), self.initial)

    def test_integration_check_retry_never_reapplies_ticket(self):
        gate = self.root / "gate"
        self.data["integration_checks"] = [
            [
                sys.executable,
                "-c",
                f"from pathlib import Path; assert Path({str(gate)!r}).exists()",
            ]
        ]
        self.add_ticket("b", ["a"], mode="need-a")
        self.assertNotEqual(self.call("run", "--jobs", "1").returncode, 0)
        state = self.state()["tasks"]
        self.assertEqual(state["a"]["status"], "blocked")
        self.assertEqual(state["b"]["status"], "pending")
        merged = state["a"]["integration_commit"]
        gate.touch()
        self.assert_passed(self.call("run", "--jobs", "1", "--retry", "a"))
        item = self.state()["tasks"]["a"]
        self.assertEqual(item["integration_commit"], merged)
        self.assertEqual(len(item["history"]), 1)
        self.assertEqual(self.git("log", "--format=%s").count("feat: implement a"), 1)

    def test_refuses_changed_graph_and_unrecorded_integration_changes(self):
        self.assert_passed(self.call("run", "--jobs", "1"))
        (self.work / "a.md").write_text("different intent")
        result = self.call("status")
        self.assertIn("graph or briefs changed", result.stderr)
        (self.work / "a.md").write_text("Deliver a. Test its behavior.\n")
        (self.repo / "repair.txt").write_text("repair")
        self.git("add", "repair.txt")
        self.git("commit", "-m", "fix: repair")
        result = self.call("run", "--jobs", "1")
        self.assertIn("HEAD drifted", result.stderr)
        self.assert_passed(
            self.call(
                "run",
                "--jobs",
                "1",
                "--accept-head",
                self.git("rev-parse", "HEAD").strip(),
            )
        )
        self.assertEqual(len(self.state()["adopted_heads"]), 1)

    def test_timeout_preserves_work_and_blocks_ticket(self):
        self.data["tickets"][0]["worker_command"][-1] = "slow"
        result = self.call("run", "--jobs", "1", "--worker-timeout", "0.2")
        self.assertIn("timed out", result.stderr)
        item = self.state()["tasks"]["a"]
        self.assertEqual(item["status"], "blocked")
        self.assertIn("timed out", item["reason"])
        self.assertTrue(Path(item["worker_path"]).is_dir())

    def test_lock_contention_and_dirty_checkout_prevent_launch(self):
        directory = self.work / "dispatch"
        directory.mkdir()
        with (self.repo / ".agent/work/.dispatch.lock").open("a+") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            result = self.call("run", "--jobs", "1")
            self.assertIn("already running", result.stderr)

        (self.repo / "README.md").write_text("uncommitted")
        result = self.call("run", "--jobs", "1")
        self.assertIn("must be clean", result.stderr)
        self.assertFalse((directory / "state.json").exists())

    def test_completed_merge_is_reconciled_after_state_write_interruption(self):
        self.assert_passed(self.call("run", "--jobs", "1"))
        state = self.state()
        head = state["integration_head"]
        item = state["tasks"]["a"]
        item["status"] = "merging"
        item.pop("integration_commit")
        item.pop("integration_checks")
        state["integration_head"] = self.initial
        (self.work / "dispatch/state.json").write_text(json.dumps(state))
        self.assert_passed(self.call("run", "--jobs", "1"))
        self.assertEqual(self.state()["tasks"]["a"]["status"], "integrated")
        self.assertEqual(self.git("rev-parse", "HEAD").strip(), head)
        self.assertEqual(self.git("log", "--format=%s").count("feat: implement a"), 1)

    def test_missing_cli_blocks_without_provider_fallback(self):
        self.data["tickets"][0]["worker_command"] = [str(self.root / "missing-cli")]
        result = self.call("run", "--jobs", "1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("No such file", result.stderr)
        self.assertEqual(self.state()["tasks"]["a"]["status"], "blocked")
        self.assertEqual(self.git("rev-parse", "HEAD").strip(), self.initial)

    def test_finished_worker_is_not_timed_out_while_integration_checks_run(self):
        self.add_ticket("b")
        self.data["integration_checks"] = [
            [sys.executable, "-c", "import time; time.sleep(2.5)"]
        ]
        self.assert_passed(self.call("run", "--jobs", "2", "--worker-timeout", "2"))
        self.assertEqual(self.state()["tasks"]["b"]["status"], "integrated")

    def test_failed_worker_children_are_stopped_before_retry(self):
        self.data["tickets"][0]["worker_command"][-1] = "child-fail"
        self.data["tickets"][0]["worker_command"].append(str(self.root / "child.pid"))
        result = self.call("run", "--jobs", "1")
        self.assertIn("worker failed (7)", result.stderr)
        child_status = subprocess.run(
            ["ps", "-p", (self.root / "child.pid").read_text(), "-o", "stat="],
            capture_output=True,
            text=True,
        ).stdout.strip()
        self.assertTrue(not child_status or child_status.startswith("Z"))
        self.runner.write_text(
            self.runner.read_text().replace("if mode == 'child-fail':", "if False:")
        )
        self.assert_passed(self.call("run", "--jobs", "1", "--retry", "a"))

    def test_unrelated_integration_edits_are_preserved_before_merge(self):
        self.data["tickets"][0]["worker_command"][-1] = "main-dirty"
        self.data["tickets"][0]["worker_command"].append(str(self.repo / "README.md"))
        result = self.call("run", "--jobs", "1")
        self.assertIn("preserve and reconcile", result.stderr)
        self.assertEqual((self.repo / "README.md").read_text(), "unrelated edit")
        self.assertEqual(self.git("rev-parse", "HEAD").strip(), self.initial)
        self.assertEqual(self.state()["tasks"]["a"]["status"], "blocked")

    def test_deadline_is_enforced_during_validation_before_new_launches(self):
        clock = self.root / "clock"
        self.add_ticket("b", mode="deadline")
        self.data["tickets"][1]["worker_command"].append(str(clock))
        self.add_ticket("c", ["a"])
        self.data["integration_checks"] = [
            [sys.executable, "-c", "import time; time.sleep(4)"]
        ]
        result = self.call("run", "--jobs", "2", "--worker-timeout", "2")
        self.assertIn("worker timed out: b", result.stderr)
        began, stopped = map(float, clock.read_text().splitlines())
        self.assertLess(stopped - began, 3)
        self.assertEqual(self.state()["tasks"]["b"]["status"], "blocked")
        self.assertEqual(self.state()["tasks"]["c"]["status"], "pending")
        self.assertNotIn('"task": "c"', result.stdout)

    def test_blocked_retry_refuses_a_live_previous_worker_group(self):
        self.data["tickets"][0]["worker_command"][-1] = "fail"
        self.assertNotEqual(self.call("run", "--jobs", "1").returncode, 0)
        process = subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(30)"],
            start_new_session=True,
        )
        try:
            state = self.state()
            state["tasks"]["a"]["pid"] = process.pid
            (self.work / "dispatch/state.json").write_text(json.dumps(state))
            result = self.call("run", "--jobs", "1", "--retry", "a")
            self.assertIn("previous worker group is still live", result.stderr)
            self.assertEqual(self.state()["tasks"]["a"]["history"], [])
        finally:
            process.terminate()
            process.wait(timeout=5)


if __name__ == "__main__":
    unittest.main()
