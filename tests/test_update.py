import fcntl
import importlib.util
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
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
        second["selection"]["include"] = [
            {"source": "skills/beta", "destination": "beta"}
        ]
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
                            "include": [
                                {"source": "skill", "destination": "local-skill"}
                            ],
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

    def test_claude_flag_is_boolean_and_requires_explicit_selection(self):
        dependencies, _ = self.module.load_manifest(self.load())
        self.assertTrue(dependencies[0]["claude"])

        discovery = explicit_dependency(claude=False)
        discovery["selection"] = {"mode": "discovery", "root": "skills"}
        cases = {
            "must be a boolean": explicit_dependency(claude="no"),
            "requires explicit selection or a router": discovery,
        }
        for message, dependency in cases.items():
            with self.subTest(message=message):
                with self.assertRaisesRegex(self.module.ManifestError, message):
                    self.module.load_manifest(
                        self.load({"version": 1, "dependencies": [dependency]})
                    )

    def test_router_validation_and_collisions(self):
        router = {"name": "vendor", "description": "Use for vendor work."}
        discovery = explicit_dependency(claude=False, router=router)
        discovery["selection"] = {"mode": "discovery", "root": "skills"}
        dependencies, _ = self.module.load_manifest(
            self.load({"version": 1, "dependencies": [discovery]})
        )
        self.assertEqual(dependencies[0]["router"], router)

        same_destination = explicit_dependency(id="other")
        cases = {
            "unknown keys": (explicit_dependency(router={**router, "x": 1}), ()),
            "description must be a string": (
                explicit_dependency(router={"name": "vendor", "description": " "}),
                (),
            ),
            "safe directory name": (
                explicit_dependency(router={**router, "name": "../x"}),
                (),
            ),
            "authored skill": (explicit_dependency(router=router), ("vendor",)),
        }
        for message, (dependency, authored) in cases.items():
            with self.subTest(message=message):
                with self.assertRaisesRegex(self.module.ManifestError, message):
                    self.module.load_manifest(
                        self.load(
                            {"version": 1, "dependencies": [dependency]},
                            authored=authored,
                        )
                    )

        nested = explicit_dependency(router=router)
        dependencies, _ = self.module.load_manifest(
            self.load({"version": 1, "dependencies": [nested, same_destination]})
        )
        self.assertEqual(len(dependencies), 2)

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
                "XDG_CACHE_HOME": str(Path(self.temporary.name) / "cache"),
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

    def commit_source(self):
        subprocess.run(["git", "-C", self.source, "add", "-A"], check=True)
        subprocess.run(
            ["git", "-C", self.source, "commit", "-qm", "Update fixture"], check=True
        )

    def test_unchanged_sync_fetches_without_copying_or_replacing(self):
        self.materialize()
        installed = self.root / "_managed" / "alpha" / "SKILL.md"
        before = installed.stat()
        with (
            mock.patch.object(self.module, "run_git", wraps=self.module.run_git) as git,
            mock.patch.object(
                self.module.shutil, "copytree", wraps=self.module.shutil.copytree
            ) as copy,
        ):
            self.materialize()

        self.assertTrue(any(c.args[0][0] == "fetch" for c in git.call_args_list))
        self.assertFalse(any(c.args[0][0] == "init" for c in git.call_args_list))
        copy.assert_not_called()
        self.assertEqual(installed.stat().st_ino, before.st_ino)
        self.assertEqual(installed.stat().st_mtime_ns, before.st_mtime_ns)

    def test_unselected_upstream_change_skips_generation(self):
        self.materialize()
        (self.source / "README.md").write_text("Unrelated change")
        self.commit_source()
        with mock.patch.object(
            self.module.shutil, "copytree", wraps=self.module.shutil.copytree
        ) as copy:
            self.materialize()

        copy.assert_not_called()

    def test_upstream_and_destination_rename_remove_old_layout(self):
        self.materialize()
        (self.source / "skills" / "alpha").rename(self.source / "skills" / "beta")
        self.commit_source()
        dependency = explicit_dependency()
        dependency["selection"]["include"] = [
            {"source": "skills/beta", "destination": "beta"}
        ]
        write_manifest(self.root, [dependency])

        output = self.materialize()

        self.assertIn("REMOVE alpha", output)
        self.assertEqual([p.name for p in (self.root / "_managed").iterdir()], ["beta"])

    def test_upstream_deletion_preserves_installed_tree(self):
        self.materialize()
        shutil.rmtree(self.source / "skills" / "alpha")
        (self.source / "README.md").write_text("Skill removed")
        self.commit_source()
        with self.assertRaisesRegex(RuntimeError, "source is not a skill"):
            self.materialize()

        self.assertTrue((self.root / "_managed" / "alpha" / "SKILL.md").is_file())

    def test_repairs_live_edits_missing_files_and_extra_directories(self):
        self.materialize()
        managed = self.root / "_managed"
        skill = managed / "alpha" / "SKILL.md"
        skill.write_text("Local edit")
        self.materialize()
        self.assertIn("name: alpha", skill.read_text())
        skill.unlink()
        (managed / "obsolete").mkdir()
        self.materialize()
        self.assertTrue(skill.is_file())
        self.assertFalse((managed / "obsolete").exists())
        git_directory = managed / "alpha/.git"
        git_directory.mkdir()
        (git_directory / "config").write_text("Local repository metadata")
        self.materialize()
        self.assertFalse(git_directory.exists())

    def test_cleared_or_corrupt_cache_recovers(self):
        self.materialize()
        cache = Path(os.environ["XDG_CACHE_HOME"]) / "skillctl"
        shutil.rmtree(cache)
        self.materialize()
        for git_dir in cache.glob("repos/*/.git"):
            shutil.rmtree(git_dir)

        self.materialize()
        self.assertTrue(list(cache.glob("repos/*/.git")))

    def test_router_change_replaces_flat_layout(self):
        self.materialize()
        write_manifest(
            self.root,
            [explicit_dependency(router={"name": "vendor", "description": "Vendor"})],
        )
        self.materialize()
        managed = self.root / "_managed"
        self.assertFalse((managed / "alpha").exists())
        self.assertTrue((managed / "vendor/skills/alpha/GUIDE.md").is_file())

    def test_corrupt_state_rebuilds_without_replacing_identical_output(self):
        self.materialize()
        installed = self.root / "_managed/alpha/SKILL.md"
        before = installed.stat().st_ino
        (self.root / ".skillctl.lock").write_text("broken")
        self.materialize()
        self.assertEqual(installed.stat().st_ino, before)

    def test_discovery_removes_renamed_upstream_skill(self):
        dependency = explicit_dependency()
        dependency["selection"] = {"mode": "discovery", "root": "skills"}
        write_manifest(self.root, [dependency])
        self.materialize()
        (self.source / "skills/alpha").rename(self.source / "skills/beta")
        self.commit_source()

        output = self.materialize()

        self.assertIn("REMOVE alpha", output)
        self.assertIn("ADD beta", output)
        self.assertFalse((self.root / "_managed/alpha").exists())

    def test_removing_all_dependencies_clears_managed_tree(self):
        self.materialize()
        (self.root / "deps.yaml").write_text("version: 1\ndependencies: []\n")
        self.materialize()
        self.assertEqual(list((self.root / "_managed").iterdir()), [])
        lock = json.loads((self.root / ".skillctl.lock").read_text())
        self.assertEqual(lock["dependencies"], [])

    def test_warm_fetch_failure_does_not_use_stale_cache(self):
        self.materialize()
        installed = self.root / "_managed/alpha/SKILL.md"
        before = installed.stat().st_ino
        lock_before = (self.root / ".skillctl.lock").read_bytes()
        self.source.rename(self.source.with_name("offline"))
        with self.assertRaises(subprocess.CalledProcessError):
            self.materialize()

        self.assertEqual(installed.stat().st_ino, before)
        self.assertEqual((self.root / ".skillctl.lock").read_bytes(), lock_before)

    def test_dry_run_does_not_advance_state(self):
        self.materialize()
        state = self.root / ".skillctl.lock"
        before = state.read_bytes()
        (self.source / "skills/alpha/SKILL.md").write_text("Updated skill")
        self.commit_source()
        self.assertIn("UPDATE alpha", self.materialize(dry_run=True))
        self.assertEqual(state.read_bytes(), before)
        self.assertIn("UPDATE alpha", self.materialize())

    def test_lock_records_resolved_dependency_and_generation_state(self):
        self.materialize()
        lock_path = self.root / ".skillctl.lock"
        lock = json.loads(lock_path.read_text())
        expected_commit = subprocess.check_output(
            ["git", "-C", self.source, "rev-parse", "HEAD"], text=True
        ).strip()
        self.assertEqual(lock["version"], 1)
        self.assertEqual(lock["dependencies"][0]["id"], "example")
        self.assertEqual(lock["dependencies"][0]["commit"], expected_commit)
        self.assertEqual(lock["dependencies"][0]["ref"], "HEAD")
        self.assertEqual(
            lock["state"]["output"],
            self.module.tree_fingerprint(self.root / "_managed"),
        )
        self.assertFalse((self.root / ".skillctl-state.json").exists())

    def test_unselected_change_updates_lock_without_replacing_output(self):
        self.materialize()
        lock_path = self.root / ".skillctl.lock"
        before = json.loads(lock_path.read_text())
        inode = lock_path.stat().st_ino
        (self.source / "README.md").write_text("Unrelated change")
        self.commit_source()
        with mock.patch.object(
            self.module.shutil, "copytree", wraps=self.module.shutil.copytree
        ) as copy:
            self.materialize()

        after = json.loads(lock_path.read_text())
        self.assertNotEqual(before["dependencies"], after["dependencies"])
        self.assertEqual(before["state"], after["state"])
        self.assertEqual(lock_path.stat().st_ino, inode)
        copy.assert_not_called()

    def test_claude_install_preserves_dependency_lock_contents(self):
        self.materialize()
        lock_path = self.root / ".skillctl.lock"
        before = lock_path.read_bytes()
        with redirect_stdout(io.StringIO()):
            self.module.install_claude(
                self.root, [], set(), self.root.parent / "claude", dry_run=False
            )

        self.assertEqual(lock_path.read_bytes(), before)

    def test_lock_write_keeps_second_process_excluded(self):
        self.materialize()
        lock_path = self.root / ".skillctl.lock"
        inode = lock_path.stat().st_ino
        with self.module.sync_lock(self.root) as lock_file:
            self.module.write_sync_lock(lock_file, [], {"output": "checked"})
            result = subprocess.run(
                [
                    sys.executable,
                    "-c",
                    "import fcntl, sys; "
                    "f = open(sys.argv[1], 'r+'); "
                    "fcntl.flock(f.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)",
                    str(lock_path),
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("BlockingIOError", result.stderr)
            self.assertEqual(lock_path.stat().st_ino, inode)

        with self.module.sync_lock(self.root):
            pass

        self.assertEqual(
            json.loads(lock_path.read_text())["state"], {"output": "checked"}
        )

    def test_selected_file_deletion_removes_installed_file(self):
        extra = self.source / "skills/alpha/obsolete.md"
        extra.write_text("Old reference")
        self.commit_source()
        self.materialize()
        extra.unlink()
        self.commit_source()
        self.materialize()
        self.assertFalse((self.root / "_managed/alpha/obsolete.md").exists())

    def test_symlink_target_outside_selected_subtree_invalidates_state(self):
        reference = self.source / "shared.md"
        reference.write_text("Original")
        (self.source / "skills/alpha/reference.md").symlink_to("../../shared.md")
        self.commit_source()
        self.materialize()
        reference.write_text("Updated reference")
        self.commit_source()
        self.materialize()
        self.assertEqual(
            (self.root / "_managed/alpha/reference.md").read_text(), "Updated reference"
        )

    def test_symlinked_source_ancestor_supports_install_and_updates(self):
        (self.source / "alias").symlink_to("skills", target_is_directory=True)
        self.commit_source()
        dependency = explicit_dependency()
        dependency["selection"]["include"][0]["source"] = "alias/alpha"
        write_manifest(self.root, [dependency])
        self.materialize()
        (self.source / "skills/alpha/SKILL.md").write_text("Updated through alias")
        self.commit_source()
        self.materialize()
        self.assertIn(
            "Updated through alias", (self.root / "_managed/alpha/SKILL.md").read_text()
        )

    def test_acquisition_runs_with_four_bounded_workers(self):
        dependencies = []
        for index in range(5):
            dependency = explicit_dependency(id=f"dep-{index}")
            dependency["selection"]["include"][0]["destination"] = f"skill-{index}"
            dependencies.append(dependency)

        write_manifest(self.root, dependencies)
        barrier = threading.Barrier(4, timeout=10)
        lock = threading.Lock()
        active = 0
        peak = 0
        acquire = self.module.acquire_dependency

        def observed_acquire(dependency, cache_root):
            nonlocal active, peak
            with lock:
                active += 1
                peak = max(peak, active)

            try:
                if dependency["id"] != "dep-4":
                    barrier.wait()

                return acquire(dependency, cache_root)
            finally:
                with lock:
                    active -= 1

        with mock.patch.object(
            self.module, "acquire_dependency", side_effect=observed_acquire
        ):
            self.materialize()

        self.assertEqual(peak, 4)
        self.assertEqual(len(list((self.root / "_managed").iterdir())), 5)

    def test_shared_cache_lock_prevents_other_worktree_mutation(self):
        self.materialize()
        cache = self.module.git_cache_root()
        with self.module.sync_lock(cache):
            with self.assertRaisesRegex(RuntimeError, "another sync is in progress"):
                self.materialize()

    def test_pinned_commit_is_supported(self):
        commit = subprocess.check_output(
            ["git", "-C", self.source, "rev-parse", "HEAD"], text=True
        ).strip()
        write_manifest(self.root, [explicit_dependency(ref=commit)])
        self.materialize()
        (self.source / "skills/alpha/SKILL.md").write_text("New version")
        self.commit_source()
        self.materialize()
        self.assertNotIn(
            "New version", (self.root / "_managed/alpha/SKILL.md").read_text()
        )

    def test_generator_change_invalidates_generation_state(self):
        self.materialize()
        generator = self.root / "generator.py"
        generator.write_text("Changed rendering implementation")
        with (
            mock.patch.object(self.module, "__file__", str(generator)),
            mock.patch.object(
                self.module.shutil, "copytree", wraps=self.module.shutil.copytree
            ) as copy,
        ):
            self.materialize()

        copy.assert_called()

    def test_sparse_path_change_uses_fresh_checkout(self):
        write_manifest(self.root, [explicit_dependency(sparse=["skills"])])
        self.materialize()
        write_manifest(self.root, [explicit_dependency(sparse=["skills/alpha"])])
        with mock.patch.object(
            self.module, "run_git", wraps=self.module.run_git
        ) as git:
            self.materialize()

        self.assertTrue(any(c.args[0][0] == "init" for c in git.call_args_list))

    def test_default_branch_change_is_followed(self):
        self.materialize()
        subprocess.run(
            ["git", "-C", self.source, "checkout", "-qb", "next"], check=True
        )
        (self.source / "skills/alpha/SKILL.md").write_text("New default branch")
        self.commit_source()
        self.materialize()
        self.assertIn(
            "New default branch", (self.root / "_managed/alpha/SKILL.md").read_text()
        )

    def test_force_updated_ref_is_followed(self):
        write_manifest(self.root, [explicit_dependency(ref="main")])
        self.materialize()
        (self.source / "skills/alpha/SKILL.md").write_text("Rewritten history")
        subprocess.run(["git", "-C", self.source, "add", "-A"], check=True)
        subprocess.run(
            ["git", "-C", self.source, "commit", "--amend", "--no-edit", "-q"],
            check=True,
        )
        self.materialize()
        self.assertIn(
            "Rewritten history", (self.root / "_managed/alpha/SKILL.md").read_text()
        )

    def test_rewrites_frontmatter_name_to_destination(self):
        dependency = explicit_dependency()
        dependency["selection"]["include"][0]["destination"] = "example-alpha"
        write_manifest(self.root, [dependency])

        self.materialize()

        content = (self.root / "_managed" / "example-alpha" / "SKILL.md").read_text()
        self.assertEqual(content, "---\nname: example-alpha\n---\n")

    def test_router_nests_skills_behind_one_index(self):
        router = {"name": "vendor", "description": "Use for  vendor\n work."}
        write_manifest(self.root, [explicit_dependency(router=router)])
        (self.root / "_managed" / "alpha").mkdir(parents=True)

        output = self.materialize()

        self.assertIn("ADD vendor", output)
        self.assertIn("REMOVE alpha", output)
        managed = self.root / "_managed"
        self.assertEqual([path.name for path in managed.iterdir()], ["vendor"])
        guide = managed / "vendor" / "skills" / "alpha"
        self.assertEqual(sorted(path.name for path in guide.iterdir()), ["GUIDE.md"])
        self.assertEqual(
            list((managed / "vendor").rglob("SKILL.md")),
            [managed / "vendor" / "SKILL.md"],
        )
        index = (managed / "vendor" / "SKILL.md").read_text()
        self.assertTrue(
            index.startswith('---\nname: vendor\ndescription: "Use for vendor work."\n')
        )
        self.assertIn("- `alpha` (`skills/alpha/GUIDE.md`)\n", index)

    def test_router_renames_nested_skills_and_local_links(self):
        skill = self.source / "skills" / "alpha"
        nested = skill / "nested"
        nested.mkdir()
        (nested / "SKILL.md").write_text("See [parent](../SKILL.md).\n")
        (skill / "SKILL.md").write_text(
            "---\nname: alpha\n---\n"
            "Read [nested](nested/SKILL.md) and `nested/SKILL.md`.\n"
            "Keep https://example.com/SKILL.md.\n"
        )
        subprocess.run(["git", "-C", str(self.source), "add", "."], check=True)
        subprocess.run(
            ["git", "-C", str(self.source), "commit", "-m", "Add nested skill"],
            check=True,
            stdout=subprocess.PIPE,
        )
        write_manifest(
            self.root,
            [
                explicit_dependency(
                    router={"name": "vendor", "description": "Use for vendor work."}
                )
            ],
        )

        self.materialize()

        router = self.root / "_managed" / "vendor"
        self.assertEqual(list(router.rglob("SKILL.md")), [router / "SKILL.md"])
        guide = router / "skills" / "alpha"
        self.assertIn("nested/GUIDE.md", (guide / "GUIDE.md").read_text())
        self.assertIn("https://example.com/SKILL.md", (guide / "GUIDE.md").read_text())
        self.assertIn("../GUIDE.md", (guide / "nested" / "GUIDE.md").read_text())

    def test_router_rejects_existing_guide_without_replacing_live_tree(self):
        skill = self.source / "skills" / "alpha"
        (skill / "GUIDE.md").write_text("Keep this reference.\n")
        subprocess.run(["git", "-C", str(self.source), "add", "."], check=True)
        subprocess.run(
            ["git", "-C", str(self.source), "commit", "-m", "Add guide collision"],
            check=True,
            stdout=subprocess.PIPE,
        )
        write_manifest(
            self.root,
            [
                explicit_dependency(
                    router={"name": "vendor", "description": "Use for vendor work."}
                )
            ],
        )
        live = self.root / "_managed"
        live.mkdir()
        (live / "keep.txt").write_text("Unchanged")

        with self.assertRaisesRegex(self.module.ManifestError, "guide already exists"):
            self.materialize()

        self.assertEqual((live / "keep.txt").read_text(), "Unchanged")

    def test_large_router_discloses_bounded_description_indexes(self):
        router = self.root / "vendor"
        members = [f"task-{i:02}" for i in range(25)]
        description = "Distinct routing detail " * 100
        for member in members:
            guide = router / "skills" / member / "GUIDE.md"
            guide.parent.mkdir(parents=True)
            guide.write_text("---\ndescription: " + description + "\n---\n")

        self.module.write_router(
            router, {"name": "vendor", "description": "Use for vendor work."}, members
        )

        entry = (router / "SKILL.md").read_text()
        self.assertLess(len(entry), 2500)
        self.assertNotIn(description, entry)
        indexes = sorted((router / "indexes").glob("*.md"))
        self.assertEqual(len(indexes), 3)
        for index in indexes:
            self.assertIn(f"indexes/{index.name}", entry)
            self.assertLessEqual(index.read_text().count("- `task-"), 12)
        for member in members:
            self.assertIn(member, entry)
            self.assertEqual(
                sum(
                    f"../skills/{member}/GUIDE.md" in index.read_text()
                    for index in indexes
                ),
                1,
            )

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
        contents = '{"version": 1, "sentinel": "keep"}'
        lock_path.write_text(contents)
        with lock_path.open("a+") as lock_file:
            fcntl.flock(lock_file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaisesRegex(RuntimeError, "^another sync is in progress$"):
                self.module.materialize(self.root, [], set(), dry_run=False)

        self.assertEqual(lock_path.read_text(), contents)
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


class SkillNameTest(unittest.TestCase):
    def setUp(self):
        self.module = load_module()
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.skill = Path(temporary.name)

    def rewrite(self, content):
        (self.skill / "SKILL.md").write_text(content, encoding="utf-8")
        self.module.set_skill_name(self.skill, "unique")
        return (self.skill / "SKILL.md").read_text(encoding="utf-8")

    def test_replaces_inserts_or_adds_frontmatter_name(self):
        cases = {
            "---\nname: review\ndescription: d\n---\nbody\nname: keep\n": (
                "---\nname: unique\ndescription: d\n---\nbody\nname: keep\n"
            ),
            "---\ndescription: d\n---\nbody\n": (
                "---\nname: unique\ndescription: d\n---\nbody\n"
            ),
            "---\n---\nbody\n": "---\nname: unique\n---\nbody\n",
            "# body\n": "---\nname: unique\n---\n# body\n",
        }
        for original, expected in cases.items():
            with self.subTest(original=original):
                self.assertEqual(self.rewrite(original), expected)


class ClaudeInstallTest(unittest.TestCase):
    def setUp(self):
        self.module = load_module()
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name) / "skills"
        self.target = Path(temporary.name) / "claude"
        for path, name in (
            (self.root / "authored", "authored"),
            (self.root / "_managed" / "vendor-review", "review"),
        ):
            path.mkdir(parents=True)
            (path / "SKILL.md").write_text(f"---\nname: {name}\n---\n")

    def install(self, dependencies=(), *, dry_run=False):
        output = io.StringIO()
        with redirect_stdout(output):
            self.module.install_claude(
                self.root, dependencies, {"authored"}, self.target, dry_run=dry_run
            )
        return output.getvalue()

    def test_skips_dependencies_hidden_from_claude(self):
        dependency = explicit_dependency(claude=False)
        dependency["selection"]["include"][0]["destination"] = "vendor-review"
        hidden = self.module.normalize_dependency(dependency, "dep", base=True)
        self.install()

        output = self.install([hidden])

        self.assertIn("REMOVE vendor-review", output)
        self.assertFalse((self.target / "vendor-review").exists())
        self.assertTrue((self.target / "authored").is_dir())

    def test_installs_flat_unique_skills_and_is_idempotent(self):
        output = self.install()

        self.assertIn("ADD authored", output)
        self.assertIn("ADD vendor-review", output)
        self.assertEqual(
            (self.target / "vendor-review" / "SKILL.md").read_text(),
            "---\nname: vendor-review\n---\n",
        )
        self.assertEqual(
            (self.target / ".skillctl-owned").read_text(), "authored\nvendor-review\n"
        )
        self.assertEqual(
            (self.root / "_managed" / "vendor-review" / "SKILL.md").read_text(),
            "---\nname: review\n---\n",
        )
        self.assertEqual(list(self.target.glob(".skillctl-stage-*")), [])

        second = self.install()
        self.assertNotIn("ADD ", second)
        self.assertNotIn("UPDATE ", second)
        self.assertNotIn("REMOVE ", second)

    def test_unchanged_install_keeps_existing_files(self):
        self.install()
        skill = self.target / "authored/SKILL.md"
        before = skill.stat()
        self.install()
        self.assertEqual(skill.stat().st_ino, before.st_ino)
        self.assertEqual(skill.stat().st_mtime_ns, before.st_mtime_ns)

    def test_updates_and_removes_only_owned_entries(self):
        self.install()
        (self.target / "synced").mkdir()
        (self.root / "authored" / "extra.md").write_text("new")
        shutil.rmtree(self.root / "_managed" / "vendor-review")

        output = self.install()

        self.assertIn("UPDATE authored", output)
        self.assertIn("REMOVE vendor-review", output)
        self.assertTrue((self.target / "authored" / "extra.md").is_file())
        self.assertFalse((self.target / "vendor-review").exists())
        self.assertTrue((self.target / "synced").is_dir())

    def test_refuses_to_replace_unowned_entry(self):
        (self.target / "authored").mkdir(parents=True)
        (self.target / "authored" / "SKILL.md").write_text("mine")

        with self.assertRaisesRegex(RuntimeError, "not managed by skillctl"):
            self.install()

        self.assertEqual((self.target / "authored" / "SKILL.md").read_text(), "mine")
        self.assertFalse((self.target / "vendor-review").exists())

    def test_dry_run_reports_without_mutating_target(self):
        output = self.install(dry_run=True)

        self.assertIn("ADD vendor-review", output)
        self.assertIn("Would install 2 skills", output)
        self.assertEqual(list(self.target.iterdir()), [])


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
        self.assertIn("install-claude", result.stdout)


if __name__ == "__main__":
    unittest.main()
