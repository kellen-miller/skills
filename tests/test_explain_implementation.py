import json
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RENDERER = ROOT / "explain-implementation" / "scripts" / "render_briefing.py"


def briefing_payload():
    source = {"path": "src/orders.py", "symbol": "place_order", "line": 1}
    return {
        "schema_version": 2,
        "title": "Order placement",
        "summary": "Places an order after reserving inventory.",
        "snapshot": {
            "repository": "example/orders",
            "base_ref": "origin/main",
        },
        "orientation": {
            "why": "Orders need one visible lifecycle.",
            "before": "Callers coordinated side effects.",
            "after": "The service owns their ordering.",
            "entry_point": source,
            "concepts": [
                {
                    "id": "reservation",
                    "name": "Reservation",
                    "explanation": "A temporary inventory claim.",
                }
            ],
        },
        "components": [
            {
                "id": "service",
                "name": "Order service",
                "role": "Coordinates the lifecycle.",
                "owns": ["Side-effect ordering"],
                "invariants": ["Reserve before persist"],
                "connects_to": [],
                "sources": [source],
            }
        ],
        "flows": [
            {
                "id": "success",
                "name": "Successful order",
                "summary": "Reservation precedes persistence.",
                "steps": [
                    {
                        "id": "place-order",
                        "component": "service",
                        "title": "Place the order",
                        "detail": "The service performs side effects in order.",
                        "source": source,
                    }
                ],
            }
        ],
        "decisions": [
            {
                "id": "service-ownership",
                "title": "One executor",
                "choice": "Service ownership",
                "reason": "Ordering is a business invariant.",
                "tradeoffs": ["The method keeps the lifecycle visible."],
                "sources": [source],
            }
        ],
        "change_recipes": [
            {
                "id": "change-validation",
                "goal": "Change order validation",
                "start_here": source,
                "steps": ["Edit the entry boundary."],
                "watch_for": ["Keep inner invariants explicit."],
                "tests": ["python -m unittest"],
            }
        ],
        "verification": [
            {
                "id": "reservation-first",
                "behavior": "Reservation happens first",
                "tests": [source],
                "commands": ["python -m unittest"],
                "evidence": ["The fixture passed."],
            }
        ],
        "questions": [
            {
                "id": "ordering-owner",
                "prompt": "Which component owns ordering?",
                "choices": ["Order service", "Caller"],
                "answer": 0,
                "explanation": "The service is the lifecycle executor.",
                "source": source,
            }
        ],
        "risks": [],
        "glossary": [
            {
                "id": "reservation",
                "term": "Reservation",
                "definition": "A temporary inventory claim.",
            }
        ],
    }


class RenderBriefingTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.repo = Path(self.temporary.name) / "repo"
        self.repo.mkdir()
        subprocess.run(
            ["git", "init", "-b", "main"],
            cwd=self.repo,
            check=True,
            stdout=subprocess.PIPE,
        )
        subprocess.run(
            ["git", "config", "user.name", "Test User"],
            cwd=self.repo,
            check=True,
        )
        subprocess.run(
            ["git", "config", "user.email", "test@example.com"],
            cwd=self.repo,
            check=True,
        )
        (self.repo / ".gitignore").write_text(".agent/work/\n", encoding="utf-8")
        source = self.repo / "src" / "orders.py"
        source.parent.mkdir()
        source.write_text("def place_order():\n    pass\n", encoding="utf-8")
        subprocess.run(["git", "add", "."], cwd=self.repo, check=True)
        subprocess.run(
            ["git", "commit", "-m", "test fixture"],
            cwd=self.repo,
            check=True,
            stdout=subprocess.PIPE,
        )
        self.input = Path(self.temporary.name) / "briefing.json"
        self.input.write_text(
            json.dumps(briefing_payload()),
            encoding="utf-8",
        )
        self.output = (
            self.repo / ".agent" / "work" / "orders" / "implementation-briefing.html"
        )

    def render(self):
        return subprocess.run(
            [
                "python3",
                str(RENDERER),
                "--input",
                str(self.input),
                "--output",
                str(self.output),
                "--repo-root",
                str(self.repo),
            ],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

    def test_renders_one_ignored_self_contained_html_file(self):
        status_before = subprocess.check_output(
            ["git", "status", "--short", "--untracked-files=all"],
            cwd=self.repo,
        )

        result = self.render()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(Path(result.stdout.strip()).resolve(), self.output.resolve())
        self.assertTrue(self.output.is_file())
        self.assertEqual(
            list(self.output.parent.iterdir()),
            [self.output],
        )
        html = self.output.read_text(encoding="utf-8")
        self.assertIn("<!doctype html>", html)
        self.assertIn('"title":"Order placement"', html)
        self.assertIn('"tree_state":"clean"', html)
        self.assertNotIn("__IMPLEMENTATION_BRIEFING_DATA__", html)
        self.assertNotIn("https://", html)
        self.assertIn("node.id = `${kind}-${id}`", html)
        self.assertIn("node.dataset.briefingKind = kind", html)
        self.assertIn("node.dataset.briefingId = id", html)
        self.assertIn('"id":"service-ownership"', html)
        self.assertIn('"id":"ordering-owner"', html)
        self.assertIn("window.lavish.queuePrompt", html)
        self.assertIn("lavish-active", html)
        self.assertIn("Copy path", html)
        self.assertIn("source-copy-value", html)
        self.assertIn("copyValue.select()", html)
        self.assertIn("file://${root}/${source.path}", html)
        self.assertEqual(
            subprocess.check_output(
                ["git", "status", "--short", "--untracked-files=all"],
                cwd=self.repo,
            ),
            status_before,
        )

    def test_rejects_unknown_component_references(self):
        payload = briefing_payload()
        payload["flows"][0]["steps"][0]["component"] = "missing"
        self.input.write_text(json.dumps(payload), encoding="utf-8")

        result = self.render()

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("references unknown id: missing", result.stderr)
        self.assertFalse(self.output.exists())

    def test_rejects_unignored_output(self):
        (self.repo / ".gitignore").write_text("", encoding="utf-8")

        result = self.render()

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("output path is not ignored by git", result.stderr)
        self.assertFalse(self.output.exists())

    def test_rejects_missing_source_paths(self):
        payload = briefing_payload()
        payload["orientation"]["entry_point"]["path"] = "src/missing.py"
        self.input.write_text(json.dumps(payload), encoding="utf-8")

        result = self.render()

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("source path does not exist: src/missing.py", result.stderr)
        self.assertFalse(self.output.exists())

    def test_rejects_invalid_stable_ids(self):
        payload = briefing_payload()
        payload["decisions"][0]["id"] = "Service Ownership"
        self.input.write_text(json.dumps(payload), encoding="utf-8")

        result = self.render()

        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "decisions[0].id must be a stable lowercase id",
            result.stderr,
        )
        self.assertFalse(self.output.exists())
