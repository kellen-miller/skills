use regex::Regex;
use serde_yaml_ng::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn tracked_paths() -> Vec<String> {
    let output = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root())
        .output()
        .unwrap();
    assert!(output.status.success());
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8(path.to_vec()).unwrap())
        .collect()
}

#[test]
fn tracked_tree_contains_only_owned_rust_and_public_skill_content() {
    let allowed = [
        ".gitignore",
        "LICENSE",
        "README.md",
        "deps.yaml",
        "Cargo.toml",
        "Cargo.lock",
        ".jscpd.json",
        "renovate.json",
        "rust-toolchain.toml",
    ];
    let prefixes = [
        ".github/workflows/",
        "adversarial-review/",
        "explain-implementation/",
        "frontend-design/",
        "grill-plan-build/",
        "dispatch-tickets/",
        "src/",
        "tests/",
        "templates/",
    ];
    let paths = tracked_paths();
    assert!(!paths.is_empty());
    for path in paths {
        assert!(
            allowed.contains(&path.as_str())
                || prefixes.iter().any(|prefix| path.starts_with(prefix)),
            "unexpected tracked path: {path}"
        );
        for component in Path::new(&path).components() {
            let component = component.as_os_str();
            assert!(
                ![
                    "_managed",
                    ".skillctl.lock",
                    "deps.local.yaml",
                    "__pycache__",
                    "target"
                ]
                .iter()
                .any(|forbidden| component == *forbidden),
                "generated/private path: {path}"
            );
        }

        assert!(
            !["py", "pyc", "pyo", "go"].contains(
                &Path::new(&path)
                    .extension()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .as_ref()
            ),
            "owned source must be Rust: {path}"
        );
    }
}

#[test]
fn readme_is_a_valid_flat_skill() {
    let content = fs::read_to_string(root().join("README.md")).unwrap();
    assert!(content.starts_with("---\n"));
    let header = content.split("---").nth(1).unwrap();
    let metadata: Value = serde_yaml_ng::from_str(header).unwrap();
    assert!(
        Regex::new("^[a-z0-9-]+$")
            .unwrap()
            .is_match(metadata["name"].as_str().unwrap())
    );
    assert!(
        metadata["description"]
            .as_str()
            .unwrap()
            .starts_with("Use when ")
    );
}

#[test]
fn public_manifest_contains_only_reviewed_public_dependencies() {
    let content = fs::read_to_string(root().join("deps.yaml")).unwrap();
    let manifest: Value = serde_yaml_ng::from_str(&content).unwrap();
    assert_eq!(manifest["version"].as_u64(), Some(1));
    let dependencies = manifest["dependencies"].as_sequence().unwrap();
    let expected: BTreeSet<_> = [
        "deburr",
        "superpowers",
        "terraform-skill",
        "cloudflare-skills",
        "datadog-pup",
        "openfga",
        "googleworkspace-cli",
        "unslop-ai-text",
        "useful-codex-skills",
        "mattpocock-skills",
        "aws-agent-toolkit",
        "azure-skills",
        "redpanda-skills",
        "grafana-gcx",
    ]
    .into_iter()
    .collect();
    let actual: BTreeSet<_> = dependencies
        .iter()
        .map(|dependency| dependency["id"].as_str().unwrap())
        .collect();
    assert_eq!(dependencies.len(), expected.len());
    assert_eq!(actual, expected);
    let public = Regex::new(r"^https://github\.com/[^/]+/[^/]+\.git$").unwrap();
    for dependency in dependencies {
        assert!(public.is_match(dependency["url"].as_str().unwrap()));
    }
}

#[test]
fn rust_tooling_replaces_python_and_go_tooling() {
    for path in [
        "src/bin/skillctl.rs",
        "src/bin/dispatch-tickets.rs",
        "Cargo.toml",
        "Cargo.lock",
    ] {
        assert!(
            root().join(path).is_file(),
            "missing Rust entry point/tooling: {path}"
        );
    }

    for path in [
        "pyproject.toml",
        "uv.lock",
        "go.mod",
        "go.sum",
        "dispatch-tickets/scripts/dispatch.py",
    ] {
        assert!(
            !root().join(path).exists(),
            "obsolete tooling remains: {path}"
        );
    }
}

#[test]
fn ci_is_pr_only_and_uses_shared_rust_and_duplication_checks() {
    let content = fs::read_to_string(root().join(".github/workflows/ci.yml")).unwrap();
    let workflow: Value = serde_yaml_ng::from_str(&content).unwrap();
    let triggers = workflow["on"].as_mapping().unwrap();
    assert!(triggers.contains_key(Value::String("pull_request".into())));
    assert!(!triggers.contains_key(Value::String("push".into())));
    let mut actions = Vec::new();
    for job in workflow["jobs"].as_mapping().unwrap().values() {
        if let Some(steps) = job["steps"].as_sequence() {
            actions.extend(steps.iter().filter_map(|step| step["uses"].as_str()));
        }
    }

    for name in ["rust-check", "code-duplication"] {
        let prefix = format!("kellen-miller/ci/.github/actions/{name}@");
        assert!(
            actions.iter().any(|action| action
                .strip_prefix(&prefix)
                .is_some_and(|reference| reference.len() == 40
                    && reference.chars().all(|c| c.is_ascii_hexdigit()))),
            "missing immutable shared action: {name}"
        );
    }

    assert!(!content.contains("setup-go") && !content.contains("setup-uv"));
}

#[test]
fn tracked_content_has_no_credentials_or_generated_markers() {
    let credentials = Regex::new(concat!(
        r"BEGIN (?:RSA|OPENSSH|EC) PRIVATE ",
        r"KEY|",
        r"gh",
        r"[pousr]_|",
        r"\.generated-by-",
        r"update\.py"
    ))
    .unwrap();
    let ssh_url = Regex::new(concat!("git", r#"@([a-zA-Z0-9._-]+):[^\s"']+"#)).unwrap();
    for path in tracked_paths() {
        let file = root().join(&path);
        if !file.is_file() {
            continue;
        }

        let content = fs::read_to_string(file).unwrap();
        assert!(
            !credentials.is_match(&content),
            "credential/generated marker in {path}"
        );
        for capture in ssh_url.captures_iter(&content) {
            let host = capture.get(1).unwrap().as_str();
            assert!(
                host.ends_with(".test") || host == "example.com",
                "private SSH repository in {path}"
            );
        }
    }
}
