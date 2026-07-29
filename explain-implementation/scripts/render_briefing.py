#!/usr/bin/env python3
"""Render one self-contained implementation briefing from structured evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
from datetime import UTC, datetime
from pathlib import Path
from typing import Any


class BriefingError(ValueError):
    """Raised when briefing evidence violates the rendering contract."""


STABLE_ID_PATTERN = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")


def run_git(repo_root: Path, *args: str) -> bytes:
    result = subprocess.run(
        ["git", *args],
        cwd=repo_root,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return result.stdout


def require_text(value: Any, location: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise BriefingError(f"{location} must be a non-empty string")
    return value.strip()


def require_stable_id(value: Any, location: str) -> str:
    stable_id = require_text(value, location)
    if not STABLE_ID_PATTERN.fullmatch(stable_id):
        raise BriefingError(f"{location} must be a stable lowercase id")
    return stable_id


def validate_source(source: Any, location: str) -> None:
    if not isinstance(source, dict):
        raise BriefingError(f"{location} must be an object")
    require_text(source.get("path"), f"{location}.path")
    for key in ("label", "symbol"):
        if key in source:
            require_text(source[key], f"{location}.{key}")
    if "line" in source and (not isinstance(source["line"], int) or source["line"] < 1):
        raise BriefingError(f"{location}.line must be a positive integer")


def source_references(data: dict[str, Any]):
    yield data["orientation"]["entry_point"]
    for component in data["components"]:
        yield from component["sources"]
    for flow in data["flows"]:
        for step in flow["steps"]:
            yield step["source"]
    for decision in data["decisions"]:
        yield from decision["sources"]
    for recipe in data["change_recipes"]:
        yield recipe["start_here"]
    for verification in data["verification"]:
        yield from verification["tests"]
    for question in data["questions"]:
        yield question["source"]


def validate_briefing(data: Any) -> dict[str, Any]:
    if not isinstance(data, dict):
        raise BriefingError("briefing input must be an object")
    if data.get("schema_version") != 2:
        raise BriefingError("schema_version must be 2")

    require_text(data.get("title"), "title")
    require_text(data.get("summary"), "summary")

    snapshot = data.get("snapshot")
    if not isinstance(snapshot, dict):
        raise BriefingError("snapshot must be an object")
    require_text(snapshot.get("repository"), "snapshot.repository")
    require_text(snapshot.get("base_ref"), "snapshot.base_ref")

    orientation = data.get("orientation")
    if not isinstance(orientation, dict):
        raise BriefingError("orientation must be an object")
    for key in ("why", "before", "after"):
        require_text(orientation.get(key), f"orientation.{key}")
    validate_source(orientation.get("entry_point"), "orientation.entry_point")

    concepts = orientation.get("concepts")
    if not isinstance(concepts, list) or not concepts:
        raise BriefingError("orientation.concepts must be a non-empty array")
    concept_ids: set[str] = set()
    for index, concept in enumerate(concepts):
        location = f"orientation.concepts[{index}]"
        if not isinstance(concept, dict):
            raise BriefingError(f"{location} must be an object")
        concept_id = require_stable_id(concept.get("id"), f"{location}.id")
        if concept_id in concept_ids:
            raise BriefingError(f"duplicate orientation concept id: {concept_id}")
        concept_ids.add(concept_id)
        require_text(concept.get("name"), f"{location}.name")
        require_text(
            concept.get("explanation"),
            f"{location}.explanation",
        )

    components = data.get("components")
    if not isinstance(components, list) or not components:
        raise BriefingError("components must be a non-empty array")
    component_ids: set[str] = set()
    for index, component in enumerate(components):
        location = f"components[{index}]"
        if not isinstance(component, dict):
            raise BriefingError(f"{location} must be an object")
        component_id = require_stable_id(component.get("id"), f"{location}.id")
        if component_id in component_ids:
            raise BriefingError(f"duplicate component id: {component_id}")
        component_ids.add(component_id)
        require_text(component.get("name"), f"{location}.name")
        require_text(component.get("role"), f"{location}.role")
        for key in ("owns", "invariants", "connects_to", "sources"):
            if not isinstance(component.get(key), list):
                raise BriefingError(f"{location}.{key} must be an array")
        for source_index, source in enumerate(component["sources"]):
            validate_source(source, f"{location}.sources[{source_index}]")

    for index, component in enumerate(components):
        for target in component["connects_to"]:
            if target not in component_ids:
                raise BriefingError(
                    f"components[{index}].connects_to references unknown id: {target}"
                )

    flows = data.get("flows")
    if not isinstance(flows, list) or not flows:
        raise BriefingError("flows must be a non-empty array")
    flow_ids: set[str] = set()
    for flow_index, flow in enumerate(flows):
        location = f"flows[{flow_index}]"
        if not isinstance(flow, dict):
            raise BriefingError(f"{location} must be an object")
        flow_id = require_stable_id(flow.get("id"), f"{location}.id")
        if flow_id in flow_ids:
            raise BriefingError(f"duplicate flow id: {flow_id}")
        flow_ids.add(flow_id)
        require_text(flow.get("name"), f"{location}.name")
        require_text(flow.get("summary"), f"{location}.summary")
        steps = flow.get("steps")
        if not isinstance(steps, list) or not steps:
            raise BriefingError(f"{location}.steps must be a non-empty array")
        step_ids: set[str] = set()
        for step_index, step in enumerate(steps):
            step_location = f"{location}.steps[{step_index}]"
            if not isinstance(step, dict):
                raise BriefingError(f"{step_location} must be an object")
            step_id = require_stable_id(step.get("id"), f"{step_location}.id")
            if step_id in step_ids:
                raise BriefingError(f"duplicate {location} step id: {step_id}")
            step_ids.add(step_id)
            component_id = require_text(
                step.get("component"), f"{step_location}.component"
            )
            if component_id not in component_ids:
                raise BriefingError(
                    f"{step_location}.component references unknown id: {component_id}"
                )
            require_text(step.get("title"), f"{step_location}.title")
            require_text(step.get("detail"), f"{step_location}.detail")
            validate_source(step.get("source"), f"{step_location}.source")

    collection_contracts = {
        "decisions": ("title", "choice", "reason"),
        "change_recipes": ("goal",),
        "verification": ("behavior",),
        "questions": ("prompt", "explanation"),
    }
    for collection_name, text_keys in collection_contracts.items():
        collection = data.get(collection_name)
        if not isinstance(collection, list) or not collection:
            raise BriefingError(f"{collection_name} must be a non-empty array")
        item_ids: set[str] = set()
        for index, item in enumerate(collection):
            location = f"{collection_name}[{index}]"
            if not isinstance(item, dict):
                raise BriefingError(f"{location} must be an object")
            item_id = require_stable_id(item.get("id"), f"{location}.id")
            if item_id in item_ids:
                raise BriefingError(f"duplicate {collection_name} id: {item_id}")
            item_ids.add(item_id)
            for key in text_keys:
                require_text(item.get(key), f"{location}.{key}")

    for index, decision in enumerate(data["decisions"]):
        if not isinstance(decision.get("tradeoffs"), list):
            raise BriefingError(f"decisions[{index}].tradeoffs must be an array")
        if not isinstance(decision.get("sources"), list):
            raise BriefingError(f"decisions[{index}].sources must be an array")
        for source_index, source in enumerate(decision["sources"]):
            validate_source(source, f"decisions[{index}].sources[{source_index}]")

    for index, recipe in enumerate(data["change_recipes"]):
        validate_source(recipe.get("start_here"), f"change_recipes[{index}].start_here")
        for key in ("steps", "watch_for", "tests"):
            if not isinstance(recipe.get(key), list):
                raise BriefingError(f"change_recipes[{index}].{key} must be an array")

    for index, verification in enumerate(data["verification"]):
        for key in ("tests", "commands", "evidence"):
            if not isinstance(verification.get(key), list):
                raise BriefingError(f"verification[{index}].{key} must be an array")
        for source_index, source in enumerate(verification["tests"]):
            validate_source(source, f"verification[{index}].tests[{source_index}]")

    for index, question in enumerate(data["questions"]):
        choices = question.get("choices")
        answer = question.get("answer")
        if not isinstance(choices, list) or len(choices) < 2:
            raise BriefingError(f"questions[{index}].choices needs at least two items")
        if not isinstance(answer, int) or not 0 <= answer < len(choices):
            raise BriefingError(f"questions[{index}].answer is outside choices")
        validate_source(question.get("source"), f"questions[{index}].source")

    for collection_name, text_keys in {
        "risks": ("title", "detail", "mitigation"),
        "glossary": ("term", "definition"),
    }.items():
        collection = data.get(collection_name, [])
        if not isinstance(collection, list):
            raise BriefingError(f"{collection_name} must be an array")
        item_ids: set[str] = set()
        for index, item in enumerate(collection):
            location = f"{collection_name}[{index}]"
            if not isinstance(item, dict):
                raise BriefingError(f"{location} must be an object")
            item_id = require_stable_id(item.get("id"), f"{location}.id")
            if item_id in item_ids:
                raise BriefingError(f"duplicate {collection_name} id: {item_id}")
            item_ids.add(item_id)
            for key in text_keys:
                require_text(item.get(key), f"{location}.{key}")

    return data


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Render an ignored implementation briefing"
    )
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--repo-root", required=True, type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repo_root = args.repo_root.resolve()
    output = args.output.resolve()
    expected_parent = repo_root / ".agent" / "work"
    if output.name != "implementation-briefing.html":
        raise BriefingError("output file must be named implementation-briefing.html")
    if output.parent.parent != expected_parent:
        raise BriefingError("output must be inside .agent/work/<slug>/")

    if (
        subprocess.run(
            ["git", "check-ignore", "--quiet", "--", str(output)],
            cwd=repo_root,
            check=False,
        ).returncode
        != 0
    ):
        raise BriefingError("output path is not ignored by git")

    status_before = run_git(repo_root, "status", "--short", "--untracked-files=all")
    data = validate_briefing(json.loads(args.input.read_text(encoding="utf-8")))
    for source in source_references(data):
        source_path = Path(source["path"])
        resolved_source = (repo_root / source_path).resolve()
        if source_path.is_absolute() or repo_root not in resolved_source.parents:
            raise BriefingError(
                f"source path must stay inside the repository: {source['path']}"
            )
        if not resolved_source.is_file():
            raise BriefingError(f"source path does not exist: {source['path']}")

    status_material = b"\0".join(
        (
            status_before,
            run_git(repo_root, "diff", "--binary", "HEAD"),
            run_git(repo_root, "diff", "--cached", "--binary", "HEAD"),
        )
    )
    snapshot = data["snapshot"]
    snapshot.update(
        {
            "repository_root": str(repo_root),
            "work_item": str(output.parent.relative_to(repo_root)),
            "branch": run_git(repo_root, "branch", "--show-current")
            .decode("utf-8")
            .strip()
            or "(detached HEAD)",
            "head": run_git(repo_root, "rev-parse", "HEAD").decode("ascii").strip(),
            "tree_state": "clean" if not status_before else "dirty",
            "fingerprint": hashlib.sha256(status_material).hexdigest(),
            "generated_at": datetime.now(UTC).isoformat(timespec="seconds"),
        }
    )

    template = (
        Path(__file__).resolve().parents[1] / "assets" / "briefing-template.html"
    ).read_text(encoding="utf-8")
    token = "__IMPLEMENTATION_BRIEFING_DATA__"
    if template.count(token) != 1:
        raise BriefingError("briefing template must contain exactly one data token")
    encoded = json.dumps(data, ensure_ascii=False, separators=(",", ":")).replace(
        "</", "<\\/"
    )
    rendered = template.replace(token, encoded)

    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(rendered, encoding="utf-8")

    status_after = run_git(repo_root, "status", "--short", "--untracked-files=all")
    if status_after != status_before:
        raise BriefingError("rendering changed git status")

    print(output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
