#!/usr/bin/env python3
from __future__ import annotations

import argparse
import contextlib
import fcntl
import json
import re
import shutil
import subprocess
import sys
import tempfile
import uuid
from pathlib import Path, PurePosixPath

import yaml


class ManifestError(ValueError):
    pass


def require_mapping(value, context):
    if not isinstance(value, dict):
        raise ManifestError(f"{context} must be a mapping")
    return value


def require_keys(value, allowed, required, context):
    unknown = set(value) - allowed
    missing = required - set(value)
    if unknown:
        raise ManifestError(f"{context} has unknown keys: {', '.join(sorted(unknown))}")
    if missing:
        raise ManifestError(f"{context} is missing keys: {', '.join(sorted(missing))}")


def safe_repo_path(value, context, *, allow_dot=False):
    if not isinstance(value, str) or not value or "\0" in value or "\\" in value:
        raise ManifestError(f"{context} must be a non-empty POSIX path")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in ("", "..") for part in value.split("/")):
        raise ManifestError(f"{context} must stay inside the repository")
    if value == ".":
        if allow_dot:
            return value
        raise ManifestError(f"{context} cannot be .")
    if any(part == "." for part in value.split("/")):
        raise ManifestError(f"{context} must be normalized")
    return path.as_posix()


def safe_name(value, context):
    if (
        not isinstance(value, str)
        or not value
        or value in (".", "..", "_managed")
        or value.startswith(".")
        or "/" in value
        or "\\" in value
        or "\0" in value
    ):
        raise ManifestError(f"{context} must be a safe directory name")
    return value


def unique_strings(value, context, *, paths=False):
    if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
        raise ManifestError(f"{context} must be a list of strings")
    normalized = [
        safe_repo_path(item, f"{context} entry")
        if paths
        else safe_name(item, f"{context} entry")
        for item in value
    ]
    if len(set(normalized)) != len(normalized):
        raise ManifestError(f"{context} contains duplicates")
    return normalized


def beneath_sparse(path, prefixes):
    return any(
        prefix == "." or path == prefix or path.startswith(f"{prefix}/")
        for prefix in prefixes
    )


def normalize_include(value, context):
    if not isinstance(value, list) or not value:
        raise ManifestError(f"{context} must be a non-empty list")
    result = []
    for index, raw in enumerate(value):
        item = require_mapping(raw, f"{context}[{index}]")
        require_keys(
            item,
            {"source", "destination"},
            {"source", "destination"},
            f"{context}[{index}]",
        )
        result.append(
            {
                "source": safe_repo_path(
                    item["source"], f"{context}[{index}].source", allow_dot=True
                ),
                "destination": safe_name(
                    item["destination"], f"{context}[{index}].destination"
                ),
            }
        )
    pairs = {(item["source"], item["destination"]) for item in result}
    destinations = {item["destination"] for item in result}
    if len(pairs) != len(result):
        raise ManifestError(f"{context} contains duplicate mappings")
    if len(destinations) != len(result):
        raise ManifestError(f"{context} contains duplicate destinations")
    return result


def normalize_dependency(raw, context, *, base):
    item = require_mapping(raw, context)
    require_keys(
        item,
        {"id", "url", "ref", "sparse", "selection", "claude", "router"},
        {"id", "url", "selection"},
        context,
    )
    dependency_id = safe_name(item["id"], f"{context}.id")
    url = item["url"]
    if not isinstance(url, str):
        raise ManifestError(f"{context}.url must be a string")
    if base:
        if not url.startswith("https://github.com/") or not url.endswith(".git"):
            raise ManifestError(f"{context}.url must be an HTTPS GitHub URL")
    elif not (
        url.startswith("https://")
        or url.startswith("ssh://")
        or url.startswith("git" + "@")
    ):
        raise ManifestError(f"{context}.url must use HTTPS or SSH")
    ref = item.get("ref")
    if ref is not None and (not isinstance(ref, str) or not ref):
        raise ManifestError(f"{context}.ref must be a non-empty string")
    sparse = item.get("sparse")
    if sparse is not None:
        sparse = unique_strings(sparse, f"{context}.sparse", paths=True)
        if not sparse:
            raise ManifestError(f"{context}.sparse must not be empty")

    claude = item.get("claude", True)
    if not isinstance(claude, bool):
        raise ManifestError(f"{context}.claude must be a boolean")

    router = item.get("router")
    if router is not None:
        router = require_mapping(router, f"{context}.router")
        require_keys(
            router,
            {"name", "description"},
            {"name", "description"},
            f"{context}.router",
        )
        description = router["description"]
        if not isinstance(description, str) or not description.strip():
            raise ManifestError(f"{context}.router.description must be a string")
        router = {
            "name": safe_name(router["name"], f"{context}.router.name"),
            "description": " ".join(description.split()),
        }

    selection = require_mapping(item["selection"], f"{context}.selection")
    mode = selection.get("mode")
    if mode == "explicit":
        require_keys(
            selection,
            {"mode", "include"},
            {"mode", "include"},
            f"{context}.selection",
        )
        include = normalize_include(
            selection["include"], f"{context}.selection.include"
        )
        if sparse:
            for mapping in include:
                if not beneath_sparse(mapping["source"], sparse):
                    raise ManifestError(
                        f"{context} source {mapping['source']} is outside sparse paths"
                    )
        normalized_selection = {"mode": mode, "include": include}
    elif mode == "discovery":
        require_keys(
            selection,
            {"mode", "root", "exclude"},
            {"mode", "root"},
            f"{context}.selection",
        )
        root = safe_repo_path(
            selection["root"], f"{context}.selection.root", allow_dot=True
        )
        exclude = unique_strings(
            selection.get("exclude", []), f"{context}.selection.exclude"
        )
        if sparse and not beneath_sparse(root, sparse):
            raise ManifestError(
                f"{context} discovery root {root} is outside sparse paths"
            )
        normalized_selection = {"mode": mode, "root": root, "exclude": exclude}
    else:
        raise ManifestError(f"{context}.selection.mode must be explicit or discovery")
    if not claude and mode != "explicit" and router is None:
        raise ManifestError(
            f"{context}.claude: false requires explicit selection or a router"
        )

    return {
        "id": dependency_id,
        "url": url,
        "ref": ref,
        "sparse": sparse,
        "selection": normalized_selection,
        "claude": claude,
        "router": router,
    }


def load_yaml(path):
    try:
        return yaml.safe_load(path.read_text(encoding="utf-8"))
    except (OSError, yaml.YAMLError) as error:
        raise ManifestError(f"cannot read {path.name}: {error}") from error


def top_level_names(dependency):
    """Names a dependency occupies directly under _managed, when known statically."""
    if dependency["router"]:
        return [dependency["router"]["name"]]
    if dependency["selection"]["mode"] == "explicit":
        return [
            mapping["destination"] for mapping in dependency["selection"]["include"]
        ]
    return []


def validate_destinations(dependencies, durable_roots):
    owners = {}
    for dependency in dependencies:
        for destination in top_level_names(dependency):
            if destination in durable_roots:
                raise ManifestError(
                    f"managed destination {destination} collides with an authored skill"
                )
            if destination in owners:
                raise ManifestError(
                    f"duplicate destination {destination} in {owners[destination]} "
                    f"and {dependency['id']}"
                )
            owners[destination] = dependency["id"]


def load_manifest(repo_root):
    base_path = repo_root / "deps.yaml"
    base = require_mapping(load_yaml(base_path), "deps.yaml")
    require_keys(
        base, {"version", "dependencies"}, {"version", "dependencies"}, "deps.yaml"
    )
    if base["version"] != 1:
        raise ManifestError("deps.yaml version must be 1")
    if not isinstance(base["dependencies"], list):
        raise ManifestError("deps.yaml dependencies must be a list")
    dependencies = [
        normalize_dependency(raw, f"deps.yaml.dependencies[{index}]", base=True)
        for index, raw in enumerate(base["dependencies"])
    ]
    ids = [dependency["id"] for dependency in dependencies]
    if len(set(ids)) != len(ids):
        raise ManifestError("deps.yaml contains duplicate dependency IDs")

    overlay_path = repo_root / "deps.local.yaml"
    if overlay_path.exists():
        overlay = require_mapping(load_yaml(overlay_path), "deps.local.yaml")
        require_keys(
            overlay,
            {"version", "add", "extend", "remove"},
            {"version"},
            "deps.local.yaml",
        )
        if overlay["version"] != 1:
            raise ManifestError("deps.local.yaml version must be 1")
        additions = overlay.get("add", [])
        extensions = overlay.get("extend", [])
        removals = overlay.get("remove", [])
        if not isinstance(additions, list) or not isinstance(extensions, list):
            raise ManifestError("deps.local.yaml add and extend must be lists")
        if not isinstance(removals, list):
            raise ManifestError("deps.local.yaml remove must be a list")
        known_ids = set(ids)
        for index, raw in enumerate(additions):
            addition = normalize_dependency(
                raw, f"deps.local.yaml.add[{index}]", base=False
            )
            if addition["id"] in known_ids:
                raise ManifestError(
                    f"deps.local.yaml add redefines dependency {addition['id']}"
                )
            known_ids.add(addition["id"])
            dependencies.append(addition)
        by_id = {dependency["id"]: dependency for dependency in dependencies}
        for index, raw in enumerate(extensions):
            extension = require_mapping(raw, f"deps.local.yaml.extend[{index}]")
            require_keys(
                extension,
                {"id", "include"},
                {"id", "include"},
                f"deps.local.yaml.extend[{index}]",
            )
            dependency_id = safe_name(
                extension["id"], f"deps.local.yaml.extend[{index}].id"
            )
            if dependency_id not in by_id:
                raise ManifestError(
                    f"deps.local.yaml extend names unknown dependency {dependency_id}"
                )
            dependency = by_id[dependency_id]
            if dependency["selection"]["mode"] != "explicit":
                raise ManifestError(
                    f"deps.local.yaml cannot extend discovery dependency {dependency_id}"
                )
            extra = normalize_include(
                extension["include"], f"deps.local.yaml.extend[{index}].include"
            )
            if dependency["sparse"]:
                for mapping in extra:
                    if not beneath_sparse(mapping["source"], dependency["sparse"]):
                        raise ManifestError(
                            f"deps.local.yaml source {mapping['source']} is outside "
                            f"{dependency_id} sparse paths"
                        )
            existing_pairs = {
                (mapping["source"], mapping["destination"])
                for mapping in dependency["selection"]["include"]
            }
            if any(
                (mapping["source"], mapping["destination"]) in existing_pairs
                for mapping in extra
            ):
                raise ManifestError(
                    f"deps.local.yaml repeats a mapping for {dependency_id}"
                )
            dependency["selection"]["include"].extend(extra)

        remove_ids = set()
        for index, item in enumerate(removals):
            dep_id = safe_name(item, f"deps.local.yaml.remove[{index}]")
            if dep_id not in set(ids):
                raise ManifestError(
                    f"deps.local.yaml remove names unknown base dependency {dep_id}"
                )
            remove_ids.add(dep_id)
        if remove_ids:
            dependencies = [d for d in dependencies if d["id"] not in remove_ids]

    durable_roots = {
        path.name
        for path in repo_root.iterdir()
        if path.is_dir() and path.name != "_managed" and (path / "SKILL.md").is_file()
    }
    validate_destinations(dependencies, durable_roots)
    return dependencies, durable_roots


def run_git(args, *, cwd=None):
    subprocess.run(
        ["git", *args],
        cwd=cwd,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )


def ensure_safe_tree(source, clone_root):
    clone_root = clone_root.resolve()
    for path in [source, *source.rglob("*")]:
        if path.is_symlink():
            try:
                path.resolve(strict=True).relative_to(clone_root)
            except (FileNotFoundError, ValueError) as error:
                raise RuntimeError(
                    f"source symlink escapes repository: {path}"
                ) from error


def trees_equal(left, right):
    left_paths = {
        path.relative_to(left)
        for path in left.rglob("*")
        if not any(part == ".git" for part in path.relative_to(left).parts)
    }
    right_paths = {
        path.relative_to(right)
        for path in right.rglob("*")
        if not any(part == ".git" for part in path.relative_to(right).parts)
    }
    if left_paths != right_paths:
        return False
    for relative in left_paths:
        left_path = left / relative
        right_path = right / relative
        if left_path.is_symlink() != right_path.is_symlink():
            return False
        if left_path.is_file() and not left_path.is_symlink():
            if left_path.read_bytes() != right_path.read_bytes():
                return False
    return True


@contextlib.contextmanager
def sync_lock(repo_root):
    lock_path = repo_root / ".skillctl.lock"
    with lock_path.open("a+") as lock_file:
        try:
            fcntl.flock(lock_file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise RuntimeError("another sync is in progress") from error

        try:
            yield
        finally:
            fcntl.flock(lock_file.fileno(), fcntl.LOCK_UN)


FRONTMATTER = re.compile(r"\A---\n(.*?)^---[ \t]*$", re.S | re.M)
NAME_LINE = re.compile(r"^name:.*$", re.M)


def set_skill_name(skill_dir, name):
    """Make the frontmatter name match the directory so flattened skills stay unique."""
    skill_file = skill_dir / "SKILL.md"
    content = skill_file.read_text(encoding="utf-8")
    name_line = f"name: {name}"
    match = FRONTMATTER.match(content)
    if match is None:
        content = f"---\n{name_line}\n---\n{content}"
    else:
        header = match.group(1)
        if NAME_LINE.search(header):
            header = NAME_LINE.sub(lambda _: name_line, header, count=1)
        else:
            header = f"{name_line}\n{header}"
        content = f"---\n{header}{content[match.end(1) :]}"
    skill_file.write_text(content, encoding="utf-8")


ROUTED_GUIDE = "GUIDE.md"


def write_router(router_dir, router, members):
    lines = [
        "---",
        f"name: {router['name']}",
        f"description: {json.dumps(router['description'])}",
        "---",
        "",
        f"# {router['name']}",
        "",
        f"This skill indexes {len(members)} skills. Pick the one that matches the "
        "task, read its guide, and follow it exactly as if it had been loaded as a "
        "skill. Resolve relative paths in a guide from that guide's directory.",
        "",
    ]
    for member in sorted(members):
        guide = router_dir / "skills" / member / ROUTED_GUIDE
        match = FRONTMATTER.match(guide.read_text(encoding="utf-8"))
        try:
            metadata = yaml.safe_load(match.group(1)) if match else None
        except yaml.YAMLError:
            metadata = None
        description = (
            metadata.get("description") if isinstance(metadata, dict) else None
        )
        summary = " ".join(str(description or "").split())
        entry = f"- `{member}` (`skills/{member}/{ROUTED_GUIDE}`)"
        lines.append(f"{entry}: {summary}" if summary else entry)
    (router_dir / "SKILL.md").write_text("\n".join(lines) + "\n", encoding="utf-8")


def materialize(repo_root, dependencies, durable_roots, *, dry_run):
    with sync_lock(repo_root):
        with tempfile.TemporaryDirectory(
            prefix=".skills-sync-", dir=repo_root.parent
        ) as temporary:
            temporary_root = Path(temporary)
            clones_root = temporary_root / "clones"
            stage = temporary_root / "_managed"
            clones_root.mkdir()
            stage.mkdir()
            selected = []
            routers = []
            destinations = set()

            for dependency in dependencies:
                clone = clones_root / dependency["id"]
                args = ["clone", "--filter=blob:none", "--depth", "1"]
                if dependency["sparse"]:
                    args.append("--sparse")
                if dependency["ref"]:
                    args.extend(["--branch", dependency["ref"]])
                args.extend([dependency["url"], str(clone)])
                run_git(args)
                if dependency["sparse"]:
                    run_git(
                        ["sparse-checkout", "set", *dependency["sparse"]],
                        cwd=clone,
                    )

                selection = dependency["selection"]
                if selection["mode"] == "explicit":
                    mappings = selection["include"]
                else:
                    root = (
                        clone if selection["root"] == "." else clone / selection["root"]
                    )
                    if not root.is_dir():
                        raise RuntimeError(
                            f"{dependency['id']} discovery root does not exist: "
                            f"{selection['root']}"
                        )
                    mappings = [
                        {
                            "source": child.relative_to(clone).as_posix(),
                            "destination": child.name,
                        }
                        for child in sorted(root.iterdir(), key=lambda path: path.name)
                        if child.is_dir()
                        and child.name not in selection["exclude"]
                        and (child / "SKILL.md").is_file()
                    ]

                router = dependency["router"]
                if router:
                    destination = router["name"]
                    if destination in durable_roots:
                        raise RuntimeError(
                            f"router {destination} collides with an authored skill"
                        )
                    if destination in destinations:
                        raise RuntimeError(f"duplicate destination {destination}")
                    destinations.add(destination)
                    routers.append((router, [m["destination"] for m in mappings]))

                for mapping in mappings:
                    destination = mapping["destination"]
                    if router:
                        relative = Path(router["name"], "skills", destination)
                    else:
                        if destination in durable_roots:
                            raise RuntimeError(
                                f"managed destination {destination} collides with "
                                "an authored skill"
                            )
                        if destination in destinations:
                            raise RuntimeError(f"duplicate destination {destination}")
                        destinations.add(destination)
                        relative = Path(destination)
                    source = (
                        clone if mapping["source"] == "." else clone / mapping["source"]
                    )
                    if not source.is_dir() or not (source / "SKILL.md").is_file():
                        raise RuntimeError(
                            f"{dependency['id']} source is not a skill: "
                            f"{mapping['source']}"
                        )
                    ensure_safe_tree(source, clone)
                    selected.append((source, relative, router is not None))

            for source, relative, routed in selected:
                shutil.copytree(
                    source,
                    stage / relative,
                    ignore=shutil.ignore_patterns(".git"),
                )
                set_skill_name(stage / relative, relative.name)
                if routed:
                    # Renamed so harnesses that search recursively for SKILL.md
                    # only discover the router.
                    (stage / relative / "SKILL.md").rename(
                        stage / relative / ROUTED_GUIDE
                    )

            for router, members in routers:
                write_router(stage / router["name"], router, members)

            current = repo_root / "_managed"
            old_names = (
                {path.name for path in current.iterdir()} if current.is_dir() else set()
            )
            new_names = {path.name for path in stage.iterdir()}
            for name in sorted(new_names - old_names):
                print(f"ADD {name}")
            for name in sorted(old_names - new_names):
                print(f"REMOVE {name}")
            for name in sorted(old_names & new_names):
                if not trees_equal(current / name, stage / name):
                    print(f"UPDATE {name}")

            if not dry_run:
                backup = repo_root / f"._managed.backup.{uuid.uuid4().hex}"
                moved_old = False
                try:
                    if current.exists():
                        current.rename(backup)
                        moved_old = True
                    stage.rename(current)
                except Exception:
                    if moved_old and backup.exists() and not current.exists():
                        backup.rename(current)
                    raise
                if backup.exists():
                    shutil.rmtree(backup)
                print(f"Installed {len(new_names)} managed skills")
            else:
                print(f"Would install {len(new_names)} managed skills")


CLAUDE_OWNED = ".skillctl-owned"


def install_claude(repo_root, dependencies, durable_roots, target, *, dry_run):
    """Copy authored and managed skills flat into a Claude Code skills directory.

    Skills from dependencies marked `claude: false` are left out. Only entries
    recorded in the ownership file are replaced or removed, so skills that
    other tools put in the target are never touched.
    """
    hidden = {
        name
        for dependency in dependencies
        if not dependency["claude"]
        for name in top_level_names(dependency)
    }
    with sync_lock(repo_root):
        managed = repo_root / "_managed"
        sources = {name: repo_root / name for name in durable_roots}
        if managed.is_dir():
            for path in managed.iterdir():
                if (
                    path.name not in hidden
                    and path.is_dir()
                    and (path / "SKILL.md").is_file()
                ):
                    sources[path.name] = path

        target.mkdir(parents=True, exist_ok=True)
        owned_path = target / CLAUDE_OWNED
        owned = (
            set(owned_path.read_text(encoding="utf-8").split())
            if owned_path.is_file()
            else set()
        )
        for name in sorted(sources):
            if (target / name).exists() and name not in owned:
                raise RuntimeError(
                    f"{target / name} exists and is not managed by skillctl"
                )

        with tempfile.TemporaryDirectory(
            prefix=".skillctl-stage-", dir=target
        ) as temporary:
            stage = Path(temporary)
            for name, source in sources.items():
                shutil.copytree(
                    source, stage / name, ignore=shutil.ignore_patterns(".git")
                )
                set_skill_name(stage / name, name)

            for name in sorted(set(sources) - owned):
                print(f"ADD {name}")
            for name in sorted(owned - set(sources)):
                print(f"REMOVE {name}")
            for name in sorted(owned & set(sources)):
                if not (target / name).is_dir() or not trees_equal(
                    target / name, stage / name
                ):
                    print(f"UPDATE {name}")

            if dry_run:
                print(f"Would install {len(sources)} skills into {target}")
                return

            # Record new names before moving them in so an interrupted run can
            # still replace them next time.
            owned_path.write_text(
                "\n".join(sorted(owned | set(sources))) + "\n", encoding="utf-8"
            )
            for name in sorted(owned - set(sources)):
                shutil.rmtree(target / name, ignore_errors=True)
            for name in sorted(sources):
                destination = target / name
                if destination.is_symlink() or destination.is_file():
                    destination.unlink()
                elif destination.exists():
                    shutil.rmtree(destination)
                (stage / name).rename(destination)

            owned_path.write_text("\n".join(sorted(sources)) + "\n", encoding="utf-8")
            print(f"Installed {len(sources)} skills into {target}")


def main(argv=None):
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    sync_parser = subparsers.add_parser("sync")
    sync_parser.add_argument("--dry-run", action="store_true")
    sync_parser.add_argument(
        "--no-claude", action="store_true", help="skip the Claude Code install"
    )
    claude_parser = subparsers.add_parser(
        "install-claude", help="copy skills flat into a Claude Code skills directory"
    )
    claude_parser.add_argument("--dry-run", action="store_true")
    for subparser in (sync_parser, claude_parser):
        subparser.add_argument(
            "--claude-target", type=Path, default=Path.home() / ".claude" / "skills"
        )
    args = parser.parse_args(argv)
    repo_root = Path(__file__).resolve().parents[2]
    try:
        dependencies, durable_roots = load_manifest(repo_root)
        if args.command == "sync":
            materialize(repo_root, dependencies, durable_roots, dry_run=args.dry_run)
        if args.command == "install-claude" or not args.no_claude:
            install_claude(
                repo_root,
                dependencies,
                durable_roots,
                args.claude_target,
                dry_run=args.dry_run,
            )
    except (
        ManifestError,
        RuntimeError,
        OSError,
        subprocess.CalledProcessError,
    ) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
