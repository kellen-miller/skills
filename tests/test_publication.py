import re
import subprocess
import unittest
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
EXPECTED_IDS = {
    "superpowers",
    "terraform-skill",
    "cloudflare-skills",
    "datadog-pup",
    "openfga",
    "googleworkspace-cli",
    "unslop-ai-text",
    "useful-codex-skills",
    "mattpocock-skills",
}
ALLOWED_FILES = {
    ".gitignore",
    "LICENSE",
    "README.md",
    "deps.yaml",
    "pyproject.toml",
    "uv.lock",
}
ALLOWED_PREFIXES = (
    ".github/workflows/",
    "adversarial-review/",
    "explain-implementation/",
    "frontend-design/",
    "grill-plan-build/",
    "src/skillctl/",
    "tests/",
)


class PublicationPolicyTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        result = subprocess.run(
            ["git", "ls-files", "-z"],
            cwd=ROOT,
            check=True,
            stdout=subprocess.PIPE,
        )
        cls.tracked = [
            path.decode("utf-8") for path in result.stdout.split(b"\0") if path
        ]

    def test_tracked_layout_contains_only_public_authored_content(self):
        self.assertTrue(
            self.tracked, "stage or commit the public tree before this test"
        )
        for path in self.tracked:
            with self.subTest(path=path):
                self.assertTrue(
                    path in ALLOWED_FILES
                    or any(path.startswith(prefix) for prefix in ALLOWED_PREFIXES),
                    f"unexpected tracked path: {path}",
                )
                self.assertNotRegex(
                    path,
                    r"(^|/)(_managed|\.skillctl\.lock|deps\.local\.yaml)(/|$)",
                )
                self.assertNotIn("__pycache__", path)
                self.assertFalse(path.endswith((".pyc", ".pyo")))

    def test_manifest_has_only_reviewed_public_dependencies(self):
        manifest = yaml.safe_load((ROOT / "deps.yaml").read_text(encoding="utf-8"))
        dependencies = manifest["dependencies"]

        self.assertEqual(manifest["version"], 1)
        self.assertEqual({item["id"] for item in dependencies}, EXPECTED_IDS)
        self.assertEqual(len(dependencies), len(EXPECTED_IDS))
        for dependency in dependencies:
            with self.subTest(dependency=dependency["id"]):
                self.assertRegex(
                    dependency["url"],
                    r"^https://github\.com/[^/]+/[^/]+\.git$",
                )

    def test_project_exposes_locked_skillctl_and_ruff(self):
        project = (ROOT / "pyproject.toml").read_text(encoding="utf-8")
        lock = (ROOT / "uv.lock").read_text(encoding="utf-8")

        self.assertIn('requires-python = ">=3.11"', project)
        self.assertIn('skillctl = "skillctl.cli:main"', project)
        self.assertIn('target-version = "py311"', project)
        self.assertIn('select = ["E4", "E7", "E9", "F", "I"]', project)
        self.assertIn('name = "ruff"', lock)

    def test_ci_runs_only_for_pull_requests(self):
        workflow = (ROOT / ".github" / "workflows" / "ci.yml").read_text(
            encoding="utf-8"
        )

        self.assertIn("on:\n  pull_request:\n", workflow)
        self.assertNotIn("\n  push:", workflow)

    def test_obsolete_script_and_requirements_are_not_tracked(self):
        obsolete_paths = {"update" + ".py", "requirements" + ".txt"}
        self.assertTrue(obsolete_paths.isdisjoint(self.tracked))

    def test_tracked_content_has_no_credentials_or_generated_markers(self):
        forbidden = re.compile(
            r"git"
            + r"@|"
            + r"BEGIN (?:RSA|OPENSSH|EC) PRIVATE "
            + r"KEY|"
            + r"gh"
            + r"[pousr]_|"
            + r"\.generated-by-"
            + r"update\.py"
        )
        for path in self.tracked:
            file_path = ROOT / path
            if not file_path.is_file():
                continue
            content = file_path.read_text(encoding="utf-8", errors="replace")
            with self.subTest(path=path):
                self.assertIsNone(forbidden.search(content))


if __name__ == "__main__":
    unittest.main()
