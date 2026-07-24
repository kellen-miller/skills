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
    "requirements.txt",
    "deps.yaml",
    "update.py",
}
ALLOWED_PREFIXES = (
    ".github/workflows/",
    "adversarial-review/",
    "frontend-design/",
    "grill-plan-build/",
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
            path.decode("utf-8")
            for path in result.stdout.split(b"\0")
            if path
        ]

    def test_tracked_layout_contains_only_public_authored_content(self):
        self.assertTrue(self.tracked, "stage or commit the public tree before this test")
        for path in self.tracked:
            with self.subTest(path=path):
                self.assertTrue(
                    path in ALLOWED_FILES
                    or any(path.startswith(prefix) for prefix in ALLOWED_PREFIXES),
                    f"unexpected tracked path: {path}",
                )
                self.assertNotRegex(
                    path,
                    r"(^|/)(_managed|\.update\.lock|deps\.local\.yaml)(/|$)",
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

    def test_tracked_content_has_no_credentials_or_generated_markers(self):
        forbidden = re.compile(
            r"git" + r"@|"
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
