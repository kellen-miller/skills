import fcntl
import importlib.util
import io
import os
import subprocess
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest import mock

import yaml

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "src" / "skillctl" / "cli.py"


def load_module():
    spec = importlib.util.spec_from_file_location("skills_update", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def explicit_dependency(**changes):
    dependency = {
        "id": "example",
        "url": "https://github.com/example/source.git",
        "selection": {
            "mode": "explicit",
            "include": [{"source": "skills/alpha", "destination": "alpha"}],
        },
    }
    dependency.update(changes)
    return dependency


def write_manifest(root, dependencies=None, **changes):
    manifest = {
        "version": 1,
        "dependencies": dependencies or [explicit_dependency()],
    }
    manifest.update(changes)
    (root / "deps.yaml").write_text(yaml.safe_dump(manifest), encoding="utf-8")


def make_source_repo(root):
    source = root / "source"
    skill = source / "skills" / "alpha"
    skill.mkdir(parents=True)
    (skill / "SKILL.md").write_text("---\nname: alpha\n---\n", encoding="utf-8")
    subprocess.run(
        ["git", "init", "-b", "main", source], check=True, stdout=subprocess.PIPE
    )
    subprocess.run(
        ["git", "-C", source, "config", "user.name", "Test User"], check=True
    )
    subprocess.run(
        ["git", "-C", source, "config", "user.email", "test@example.com"], check=True
    )
    subprocess.run(["git", "-C", source, "add", "."], check=True)
    subprocess.run(
        ["git", "-C", source, "commit", "-m", "test fixture"],
        check=True,
        stdout=subprocess.PIPE,
    )
    return source


class ManifestTest(unittest.TestCase):
    def setUp(self):
        self.module = load_module()

    def load(self, manifest=None, overlay=None, authored=()):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        write_manifest(root)
        if manifest is not None:
            (root / "deps.yaml").write_text(yaml.safe_dump(manifest), encoding="utf-8")
        if overlay is not None:
            (root / "deps.local.yaml").write_text(
                yaml.safe_dump(overlay), encoding="utf-8"
            )
        for name in authored:
            skill = root / name
            skill.mkdir()
            (skill / "SKILL.md").write_text("---\n", encoding="utf-8")
        return root

    def test_loads_explicit_and_discovery_modes(self):
        discovery = {
            "id": "discover",
            "url": "https://github.com/example/discover.git",
            "sparse": ["skills"],
            "selection": {
                "mode": "discovery",
                "root": "skills",
                "exclude": ["skip"],
            },
        }
        root = self.load(
            {"version": 1, "dependencies": [explicit_dependency(), discovery]}
        )

        dependencies, durable = self.module.load_manifest(root)

        self.assertEqual([item["id"] for item in dependencies], ["example", "discover"])
        self.assertEqual(dependencies[1]["selection"]["exclude"], ["skip"])
        self.assertEqual(durable, set())

    def test_rejects_unknown_keys_at_every_schema_level(self):
        mutations = []
        base = {"version": 1, "dependencies": [explicit_dependency()]}
        mutations.append({**base, "mystery": True})
        dependency = explicit_dependency(mystery=True)
        mutations.append({"version": 1, "dependencies": [dependency]})
        dependency = explicit_dependency()
        dependency["selection"]["mystery"] = True
        mutations.append({"version": 1, "dependencies": [dependency]})
        dependency = explicit_dependency()
        dependency["selection"]["include"][0]["mystery"] = True
        mutations.append({"version": 1, "dependencies": [dependency]})

        for manifest in mutations:
            with self.subTest(manifest=manifest):
                with self.assertRaisesRegex(self.module.ManifestError, "unknown keys"):
                    self.module.load_manifest(self.load(manifest))

    def test_rejects_mode_illegal_and_missing_fields(self):
        cases = []
        explicit = explicit_dependency()
        explicit["selection"]["root"] = "skills"
        cases.append(explicit)
        discovery = explicit_dependency()
        discovery["selection"] = {"mode": "discovery", "root": "skills", "include": []}
        cases.append(discovery)
        missing_include = explicit_dependency()
        missing_include["selection"] = {"mode": "explicit"}
        cases.append(missing_include)
        missing_root = explicit_dependency()
        missing_root["selection"] = {"mode": "discovery"}
        cases.append(missing_root)

        for dependency in cases:
            with self.subTest(dependency=dependency):
                with self.assertRaises(self.module.ManifestError):
                    self.module.load_manifest(
                        self.load({"version": 1, "dependencies": [dependency]})
                    )

    def test_rejects_duplicate_ids_and_destinations(self):
        duplicate = explicit_dependency()
        with self.assertRaisesRegex(
            self.module.ManifestError, "duplicate dependency IDs"
        ):
            self.module.load_manifest(
                self.load({"version": 1, "dependencies": [duplicate, duplicate]})
            )

        second = explicit_dependency(id="second")
        with self.assertRaisesRegex(self.module.ManifestError, "duplicate destination"):
            self.module.load_manifest(
                self.load({"version": 1, "dependencies": [duplicate, second]})
            )

    def test_rejects_unsafe_ids_before_acquisition_or_mutation(self):
        for dependency_id in (
            "../escape",
            "nested/name",
            "nested\\name",
            ".hidden",
            "..",
        ):
            root = self.load(
                {
                    "version": 1,
                    "dependencies": [explicit_dependency(id=dependency_id)],
                }
            )
            managed = root / "_managed"
            managed.mkdir()
            marker = managed / "keep.txt"
            marker.write_text("unchanged\n", encoding="utf-8")

            with self.subTest(dependency_id=dependency_id):
                with mock.patch.object(self.module, "run_git") as run_git:
                    with self.assertRaises(self.module.ManifestError):
                        dependencies, durable = self.module.load_manifest(root)
                        self.module.materialize(
                            root, dependencies, durable, dry_run=False
                        )

                run_git.assert_not_called()
                self.assertEqual(marker.read_text(encoding="utf-8"), "unchanged\n")

    def test_rejects_unsafe_paths_and_names(self):
        invalid_sources = ["/absolute", "../escape", "a\\b", "a/./b", ""]
        for source in invalid_sources:
            dependency = explicit_dependency()
            dependency["selection"]["include"][0]["source"] = source
            with self.subTest(source=source):
                with self.assertRaises(self.module.ManifestError):
                    self.module.load_manifest(
                        self.load({"version": 1, "dependencies": [dependency]})
                    )

        for destination in [".hidden", "..", "_managed", "a/b", "a\\b"]:
            dependency = explicit_dependency()
            dependency["selection"]["include"][0]["destination"] = destination
            with self.subTest(destination=destination):
                with self.assertRaises(self.module.ManifestError):
                    self.module.load_manifest(
                        self.load({"version": 1, "dependencies": [dependency]})
                    )

    def test_sparse_paths_must_contain_selection(self):
        dependency = explicit_dependency(sparse=["other"])
        with self.assertRaisesRegex(self.module.ManifestError, "outside sparse paths"):
            self.module.load_manifest(
                self.load({"version": 1, "dependencies": [dependency]})
            )

        dependency = {
            "id": "discover",
            "url": "https://github.com/example/discover.git",
            "sparse": ["other"],
            "selection": {"mode": "discovery", "root": "skills"},
        }
        with self.assertRaisesRegex(self.module.ManifestError, "outside sparse paths"):
            self.module.load_manifest(
                self.load({"version": 1, "dependencies": [dependency]})
            )

    def test_overlay_adds_and_extends_only(self):
        overlay = {
            "version": 1,
            "add": [
                {
                    "id": "local",
                    "url": "ssh://source@example.test/local.git",
                    "selection": {
                        "mode": "explicit",
                        "include": [{"source": "skill", "destination": "local-skill"}],
                    },
                }
            ],
            "extend": [
                {
                    "id": "example",
                    "include": [{"source": "skills/beta", "destination": "beta"}],
                }
            ],
        }
        root = self.load(overlay=overlay)

        dependencies, _ = self.module.load_manifest(root)

        self.assertEqual([item["id"] for item in dependencies], ["example", "local"])
        self.assertEqual(
            [item["destination"] for item in dependencies[0]["selection"]["include"]],
            ["alpha", "beta"],
        )

    def test_overlay_rejects_redefinition_override_and_discovery_extension(self):
        cases = [
            {"version": 1, "add": [explicit_dependency()]},
            {
                "version": 1,
                "extend": [
                    {
                        "id": "example",
                        "url": "https://example.test/replacement.git",
                        "include": [{"source": "x", "destination": "x"}],
                    }
                ],
            },
            {
                "version": 1,
                "extend": [
                    {
                        "id": "missing",
                        "include": [{"source": "x", "destination": "x"}],
                    }
                ],
            },
        ]
        for overlay in cases:
            with self.subTest(overlay=overlay):
                with self.assertRaises(self.module.ManifestError):
                    self.module.load_manifest(self.load(overlay=overlay))

        dependency = {
            "id": "discover",
            "url": "https://github.com/example/discover.git",
            "selection": {"mode": "discovery", "root": "."},
        }
        root = self.load(
            {"version": 1, "dependencies": [dependency]},
            {
                "version": 1,
                "extend": [
                    {"id": "discover", "include": [{"source": "x", "destination": "x"}]}
                ],
            },
        )
        with self.assertRaisesRegex(
            self.module.ManifestError, "cannot extend discovery"
        ):
            self.module.load_manifest(root)

    def test_overlay_removes_base_dependency(self):
        second = explicit_dependency(
            id="second",
            url="https://github.com/example/second.git",
        )
        second["selection"]["include"] = [{"source": "skills/beta", "destination": "beta"}]
        overlay = {"version": 1, "remove": ["example"]}
        root = self.load(
            {"version": 1, "dependencies": [explicit_dependency(), second]},
            overlay,
        )

        dependencies, _ = self.module.load_manifest(root)

        self.assertEqual([item["id"] for item in dependencies], ["second"])

    def test_overlay_remove_rejects_unknown_and_locally_added_ids(self):
        cases = [
            {"version": 1, "remove": ["missing"]},
            {
                "version": 1,
                "add": [
                    {
                        "id": "local",
                        "url": "ssh://source@example.test/local.git",
                        "selection": {
                            "mode": "explicit",
                            "include": [{"source": "skill", "destination": "local-skill"}],
                        },
                    }
                ],
                "remove": ["local"],
            },
        ]
        for overlay in cases:
            with self.subTest(overlay=overlay):
                with self.assertRaises(self.module.ManifestError):
                    self.module.load_manifest(self.load(overlay=overlay))

    def test_detects_authored_roots_without_hardcoded_names(self):
        root = self.load(authored=("custom-authored",))
        for name in ("_managed", ".github", "tests", "ordinary"):
            (root / name).mkdir(exist_ok=True)
        (root / "_managed" / "SKILL.md").write_text("---\n", encoding="utf-8")

        dependencies, durable = self.module.load_manifest(root)

        self.assertEqual(durable, {"custom-authored"})
        self.assertEqual(dependencies[0]["id"], "example")

    def test_authored_collision_fails_before_acquisition(self):
        root = self.load(authored=("alpha",))
        with mock.patch.object(self.module, "run_git") as run_git:
            with self.assertRaisesRegex(self.module.ManifestError, "authored skill"):
                self.module.load_manifest(root)
        run_git.assert_not_called()


class MaterializationTest(unittest.TestCase):
    def setUp(self):
        self.module = load_module()
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name) / "skills"
        self.root.mkdir()
        self.source = make_source_repo(Path(self.temporary.name))
        write_manifest(self.root)
        self.environment = mock.patch.dict(
            os.environ,
            {
                "GIT_CONFIG_COUNT": "1",
                "GIT_CONFIG_KEY_0": "url.file://" + str(self.source) + ".insteadOf",
                "GIT_CONFIG_VALUE_0": "https://github.com/example/source.git",
            },
        )
        self.environment.start()
        self.addCleanup(self.environment.stop)

    def materialize(self, *, dry_run=False):
        dependencies, durable = self.module.load_manifest(self.root)
        output = io.StringIO()
        with redirect_stdout(output):
            self.module.materialize(self.root, dependencies, durable, dry_run=dry_run)
        return output.getvalue()

    def test_installs_updates_removes_and_is_idempotent(self):
        old = self.root / "_managed"
        (old / "removed").mkdir(parents=True)
        (old / "removed" / "SKILL.md").write_text("old\n", encoding="utf-8")
        (old / "alpha").mkdir()
        (old / "alpha" / "SKILL.md").write_text("old\n", encoding="utf-8")

        output = self.materialize()

        self.assertIn("REMOVE removed", output)
        self.assertIn("UPDATE alpha", output)
        self.assertIn("Installed 1 managed skills", output)
        self.assertIn("name: alpha", (old / "alpha" / "SKILL.md").read_text())

        second = self.materialize()
        self.assertNotIn("ADD ", second)
        self.assertNotIn("REMOVE ", second)
        self.assertNotIn("UPDATE ", second)

    def test_dry_run_stages_without_mutating_live_tree(self):
        old = self.root / "_managed"
        old.mkdir()
        (old / "keep.txt").write_text("unchanged", encoding="utf-8")

        output = self.materialize(dry_run=True)

        self.assertIn("ADD alpha", output)
        self.assertIn("Would install 1 managed skills", output)
        self.assertEqual((old / "keep.txt").read_text(), "unchanged")

    def test_lock_contention_fails_without_staging_or_mutation(self):
        lock_path = self.root / ".skillctl.lock"
        with lock_path.open("a+") as lock_file:
            fcntl.flock(lock_file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaisesRegex(RuntimeError, "^another sync is in progress$"):
                self.module.materialize(self.root, [], set(), dry_run=False)
        self.assertFalse((self.root / "_managed").exists())
        self.assertEqual(
            list(self.root.parent.glob(".skills-sync-*")),
            [],
        )

    def test_acquisition_failure_preserves_old_tree(self):
        old = self.root / "_managed"
        old.mkdir()
        (old / "keep.txt").write_text("unchanged", encoding="utf-8")
        dependency = explicit_dependency(ref="missing-ref")
        write_manifest(self.root, [dependency])

        with self.assertRaises(subprocess.CalledProcessError):
            self.materialize()

        self.assertEqual((old / "keep.txt").read_text(), "unchanged")

    def test_install_failure_restores_old_tree(self):
        old = self.root / "_managed"
        old.mkdir()
        (old / "keep.txt").write_text("unchanged", encoding="utf-8")
        original_rename = Path.rename

        def fail_stage_rename(path, target):
            if path.name == "_managed" and path.parent.name.startswith(".skills-sync-"):
                raise OSError("injected install failure")
            return original_rename(path, target)

        with mock.patch.object(Path, "rename", new=fail_stage_rename):
            with self.assertRaisesRegex(OSError, "injected install failure"):
                self.materialize()

        self.assertEqual((old / "keep.txt").read_text(), "unchanged")
        self.assertEqual(
            list(self.root.glob("._managed.backup.*")),
            [],
        )


class ConsoleEntryPointTest(unittest.TestCase):
    def test_packaged_skillctl_help_is_invokable(self):
        result = subprocess.run(
            ["uv", "run", "--locked", "skillctl", "--help"],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("usage: skillctl", result.stdout)
        self.assertIn("sync", result.stdout)


if __name__ == "__main__":
    unittest.main()
