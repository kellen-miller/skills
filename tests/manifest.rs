use anyhow::Result;
use serde_json::{Value, json};
use skillctl::manifest::load_manifest;
use std::fs;
use tempfile::TempDir;

fn dependency() -> Value {
    json!({"id":"example", "url":"https://github.com/example/example.git", "selection":{"mode":"explicit", "include":[{"source":"skills/alpha", "destination":"alpha"}]}})
}

fn fixture(dependencies: Value, overlay: Option<Value>) -> Result<TempDir> {
    let root = tempfile::tempdir()?;
    fs::write(
        root.path().join("deps.yaml"),
        serde_json::to_string_pretty(&json!({"version":1, "dependencies":dependencies}))?,
    )?;
    if let Some(overlay) = overlay {
        fs::write(
            root.path().join("deps.local.yaml"),
            serde_json::to_string_pretty(&overlay)?,
        )?;
    }

    Ok(root)
}

fn reject_invalid_dependency_field(pointer: &str, value: Value, expected: &str) -> Result<()> {
    let mut dep = dependency();
    let (parent_pointer, key) = pointer
        .rsplit_once('/')
        .ok_or_else(|| anyhow::anyhow!("invalid JSON pointer: {pointer}"))?;
    let parent = if parent_pointer.is_empty() {
        &mut dep
    } else {
        dep.pointer_mut(parent_pointer)
            .ok_or_else(|| anyhow::anyhow!("missing JSON pointer parent: {parent_pointer}"))?
    };
    parent
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("JSON pointer parent is not an object: {parent_pointer}"))?
        .insert(key.into(), value);
    let root = fixture(json!([dep]), None)?;
    fs::create_dir(root.path().join("_managed"))?;
    let marker = root.path().join("_managed/keep.txt");
    fs::write(&marker, "unchanged")?;
    let error = match load_manifest(root.path()) {
        Ok(_) => anyhow::bail!("accepted invalid dependency field: {pointer}"),
        Err(error) => error,
    };
    anyhow::ensure!(
        error.to_string().contains(expected),
        "{pointer}: {error}, expected {expected}"
    );
    anyhow::ensure!(
        fs::read_to_string(marker)? == "unchanged",
        "invalid manifest changed managed output"
    );
    Ok(())
}

#[test]
fn load_modes_defaults_and_authored_roots() -> Result<()> {
    let mut explicit = dependency();
    explicit["ref"] = Value::Null;
    explicit["sparse"] = Value::Null;
    explicit["router"] = Value::Null;
    let discovery = json!({"id":"discover", "url":"https://github.com/example/discover.git", "sparse":["skills"], "selection":{"mode":"discovery", "root":"skills", "exclude":["skip"]}});
    let root = fixture(json!([explicit, discovery]), None)?;
    for name in ["z-authored", "a-authored", "_managed"] {
        fs::create_dir(root.path().join(name)).unwrap();
        fs::write(root.path().join(name).join("SKILL.md"), "---\n").unwrap();
    }

    let manifest = load_manifest(root.path()).unwrap();
    assert_eq!(manifest.durable_roots, ["a-authored", "z-authored"]);
    assert!(manifest.dependencies[0].claude);
    assert!(manifest.dependencies[0].reference.is_empty());
    assert!(manifest.dependencies[0].sparse.is_empty());
    assert_eq!(manifest.dependencies[1].selection.root, "skills");
    assert_eq!(manifest.dependencies[1].selection.exclude, ["skip"]);

    Ok(())
}

#[test]
fn rejects_invalid_dependency_fields_without_mutating_managed_output() -> Result<()> {
    let cases = [
        ("/unknown", json!(true), "unknown keys"),
        ("/id", json!(12), "safe directory name"),
        ("/url", json!(false), "must be a string"),
        ("/url", json!("git@example.test/repo.git"), "HTTPS GitHub URL"),
        ("/url", json!("https://example.test/repo.git"), "HTTPS GitHub URL"),
        ("/url", json!("https://github.com/example/repo"), "HTTPS GitHub URL"),
        ("/ref", json!(12), "non-empty string"),
        ("/ref", json!(""), "non-empty string"),
        ("/claude", json!("no"), "must be a boolean"),
        ("/claude", Value::Null, "must be a boolean"),
        ("/claude", json!(1), "must be a boolean"),
        ("/sparse", json!("skills"), "list of strings"),
        ("/sparse", json!([12]), "list of strings"),
        ("/sparse", json!([]), "must not be empty"),
        ("/sparse", json!(["skills", "skills"]), "contains duplicates"),
        ("/sparse", json!(["."]), "cannot be ."),
        ("/sparse", json!(["other"]), "outside sparse paths"),
        ("/sparse", json!(["skill"]), "outside sparse paths"),
        ("/selection", Value::Null, "must be a mapping"),
        ("/selection/unknown", json!(true), "unknown keys"),
        ("/selection/root", json!("skills"), "unknown keys"),
        ("/selection/mode", json!(true), "explicit or discovery"),
        ("/selection/include", json!([]), "non-empty list"),
        ("/selection/include", Value::Null, "non-empty list"),
        ("/selection/include/0/unknown", json!(true), "unknown keys"),
        (
            "/selection/include",
            json!([{"source":"skills/alpha","destination":"alpha"}, {"source":"skills/alpha","destination":"alpha"}]),
            "duplicate mappings",
        ),
        (
            "/selection/include",
            json!([{"source":"skills/alpha","destination":"alpha"}, {"source":"skills/beta","destination":"alpha"}]),
            "duplicate destinations",
        ),
        (
            "/router",
            json!({"name":"vendor","description":"Vendor","unknown":true}),
            "unknown keys",
        ),
        ("/router", json!({"name":"vendor"}), "missing keys"),
        (
            "/router",
            json!({"name":"vendor","description":" \n "}),
            "description must be a string",
        ),
        (
            "/router",
            json!({"name":"vendor","description":12}),
            "description must be a string",
        ),
        (
            "/router",
            json!({"name":"../x","description":"Vendor"}),
            "safe directory name",
        ),
    ];
    for (pointer, value, expected) in cases {
        reject_invalid_dependency_field(pointer, value, expected)?;
    }

    for field in ["id", "url", "selection"] {
        let mut dep = dependency();
        dep.as_object_mut().unwrap().remove(field);
        let root = fixture(json!([dep]), None)?;
        assert!(
            load_manifest(root.path())
                .unwrap_err()
                .to_string()
                .contains("missing keys")
        );
    }

    Ok(())
}

#[test]
fn rejects_unsafe_paths_and_names() -> Result<()> {
    for source in [
        json!("/absolute"),
        json!("../escape"),
        json!("a\\b"),
        json!("a/./b"),
        json!(""),
        json!("a//b"),
        json!("a/"),
        json!("a/../b"),
        json!("a\0b"),
        json!(12),
        json!(false),
    ] {
        let mut dep = dependency();
        dep["selection"]["include"][0]["source"] = source.clone();
        let root = fixture(json!([dep]), None)?;
        assert!(load_manifest(root.path()).is_err(), "accepted source {source}");
    }

    for name in [
        json!("../escape"),
        json!("nested/name"),
        json!("nested\\name"),
        json!(".hidden"),
        json!(".."),
        json!("."),
        json!("_managed"),
        json!(""),
        json!("a\0b"),
        json!(12),
        json!(false),
    ] {
        for field in ["id", "destination"] {
            let mut dep = dependency();
            if field == "id" {
                dep["id"] = name.clone();
            } else {
                dep["selection"]["include"][0]["destination"] = name.clone();
            }

            let root = fixture(json!([dep]), None)?;
            assert!(load_manifest(root.path()).is_err(), "accepted {field}: {name}");
        }
    }

    Ok(())
}

#[test]
fn discovery_validation_and_router_normalization() -> Result<()> {
    for (selection, extra, expected) in [
        (
            json!({"mode":"discovery","root":"skills","include":[]}),
            json!({}),
            "unknown keys",
        ),
        (json!({"mode":"discovery"}), json!({}), "missing keys"),
        (
            json!({"mode":"discovery","root":"skills","exclude":null}),
            json!({}),
            "list of strings",
        ),
        (
            json!({"mode":"discovery","root":"skills","exclude":["a","a"]}),
            json!({}),
            "contains duplicates",
        ),
        (json!({"mode":"discovery","root":12}), json!({}), "POSIX path"),
        (
            json!({"mode":"discovery","root":"skills"}),
            json!({"sparse":["other"]}),
            "outside sparse paths",
        ),
        (
            json!({"mode":"discovery","root":"skills"}),
            json!({"claude":false}),
            "requires explicit selection or a router",
        ),
    ] {
        let mut dep = dependency();
        dep["selection"] = selection;
        dep.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let root = fixture(json!([dep]), None)?;
        assert!(load_manifest(root.path()).unwrap_err().to_string().contains(expected));
    }

    let mut dep = dependency();
    dep["selection"] = json!({"mode":"discovery","root":"."});
    dep["claude"] = json!(false);
    dep["router"] = json!({"name":"vendor","description":"Use for  vendor\n work."});
    let root = fixture(json!([dep]), None)?;
    let manifest = load_manifest(root.path()).unwrap();
    assert_eq!(
        manifest.dependencies[0].router.as_ref().unwrap().description,
        "Use for vendor work."
    );
    assert!(!manifest.dependencies[0].claude);

    Ok(())
}

#[test]
fn overlay_add_extend_remove_and_local_transports() -> Result<()> {
    for url in [
        "git@example.test:local.git",
        "https://private.test/repo",
        "ssh://source@example.test/local.git",
    ] {
        let mut second = dependency();
        second["id"] = json!("removed");
        let local = json!({"id":"local", "url":url, "selection":{"mode":"explicit","include":[{"source":".","destination":"local"}]}});
        let overlay = json!({"version":1, "add":[local], "remove":["removed","removed"], "extend":[{"id":"example","include":[{"source":"skills/beta","destination":"beta"}]}]});
        let root = fixture(json!([dependency(), second]), Some(overlay))?;
        let manifest = load_manifest(root.path()).unwrap();
        assert_eq!(manifest.dependencies.len(), 2);
        assert_eq!(manifest.dependencies[1].id, "local");
        assert_eq!(manifest.dependencies[0].selection.include[1].destination, "beta");
    }

    Ok(())
}

#[test]
fn overlay_and_destination_validation() -> Result<()> {
    for (overlay, expected) in [
        (json!({"version":1,"unknown":true}), "unknown keys"),
        (json!({}), "missing keys"),
        (json!({"version":2}), "version must be 1"),
        (json!({"version":1,"add":null}), "add and extend must be lists"),
        (json!({"version":1,"extend":null}), "add and extend must be lists"),
        (json!({"version":1,"remove":null}), "remove must be a list"),
        (json!({"version":1,"add":[dependency()]}), "redefines dependency"),
        (
            json!({"version":1,"extend":[{"id":"missing","include":[{"source":"skills/beta","destination":"beta"}]}]}),
            "unknown dependency",
        ),
        (
            json!({"version":1,"extend":[{"id":"example","url":"https://example.test","include":[]}]}),
            "unknown keys",
        ),
        (json!({"version":1,"extend":[{"id":"example"}]}), "missing keys"),
        (
            json!({"version":1,"extend":[{"id":"example","include":[{"source":"skills/alpha","destination":"alpha"}]}]}),
            "repeats a mapping",
        ),
        (
            json!({"version":1,"extend":[{"id":"example","include":[{"source":"skills/beta","destination":"alpha"}]}]}),
            "duplicate destination",
        ),
        (json!({"version":1,"remove":["missing"]}), "unknown base dependency"),
        (json!({"version":1,"remove":["../escape"]}), "safe directory name"),
    ] {
        let root = fixture(json!([dependency()]), Some(overlay))?;
        assert!(
            load_manifest(root.path()).unwrap_err().to_string().contains(expected),
            "expected {expected}"
        );
    }

    let root = fixture(json!([dependency(), dependency()]), None)?;
    assert!(
        load_manifest(root.path())
            .unwrap_err()
            .to_string()
            .contains("duplicate dependency IDs")
    );
    let mut second = dependency();
    second["id"] = json!("second");
    let root = fixture(json!([dependency(), second]), None)?;
    assert!(
        load_manifest(root.path())
            .unwrap_err()
            .to_string()
            .contains("duplicate destination")
    );
    let root = fixture(json!([dependency()]), None)?;
    fs::create_dir(root.path().join("alpha")).unwrap();
    fs::write(root.path().join("alpha/SKILL.md"), "authored").unwrap();
    assert!(
        load_manifest(root.path())
            .unwrap_err()
            .to_string()
            .contains("authored skill")
    );

    Ok(())
}

#[test]
fn strict_yaml_documents() -> Result<()> {
    for (source, expected) in [
        ("", "must be a mapping"),
        ("[]", "must be a mapping"),
        ("version: 1\ndependencies: []\nunknown: true\n", "unknown keys"),
        ("version: 1\n", "missing keys"),
        ("version: '1'\ndependencies: []\n", "version must be 1"),
        ("version: 2\ndependencies: []\n", "version must be 1"),
        ("version: 1\ndependencies: null\n", "dependencies must be a list"),
        ("version: 1\ndependencies: [null]\n", "must be a mapping"),
        (
            "version: 1\ndependencies: []\n---\nversion: 1\ndependencies: []\n",
            "single document",
        ),
        ("version: [\n", "cannot read deps.yaml"),
        ("version: 1\nversion: 1\ndependencies: []\n", "cannot read deps.yaml"),
    ] {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("deps.yaml"), source).unwrap();
        let error = load_manifest(root.path()).unwrap_err().to_string();
        assert!(error.contains(expected), "source {source:?}: {error}");
    }

    Ok(())
}

#[test]
fn yaml_legacy_booleans_preserve_quotes_tags_aliases_and_unicode_positions() -> Result<()> {
    for token in [
        "yes",
        "Yes",
        "YES",
        "on",
        "On",
        "ON",
        "no",
        "No",
        "NO",
        "off",
        "Off",
        "OFF",
        "\"no\"",
        "'no'",
        "!!str no",
        "yEs",
        "|\n      no",
    ] {
        let root = tempfile::tempdir().unwrap();
        let source = format!(
            "version: 1\ndependencies:\n  - id: café\n    url: https://github.com/example/example.git\n    claude: {token}\n    selection: {{mode: explicit, include: [{{source: skill, destination: alpha}}]}}\n"
        );
        fs::write(root.path().join("deps.yaml"), source).unwrap();
        let manifest = load_manifest(root.path());
        if token.contains(['\"', '\'', '!', '|']) || token == "yEs" {
            assert!(
                manifest.unwrap_err().to_string().contains("must be a boolean"),
                "{token}"
            );
        } else {
            assert_eq!(
                manifest.unwrap().dependencies[0].claude,
                token.eq_ignore_ascii_case("yes") || token.eq_ignore_ascii_case("on"),
                "{token}"
            );
        }
    }

    for token in ["no", "ON", "true", "123"] {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("deps.yaml"), format!("version: 1\ndependencies: [{{id: {token}, url: https://github.com/a/b.git, selection: {{mode: explicit, include: [{{source: skill, destination: alpha}}]}}}}]")).unwrap();
        assert!(
            load_manifest(root.path())
                .unwrap_err()
                .to_string()
                .contains("safe directory name")
        );
    }

    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("deps.yaml"), "version: 1\ndependencies:\n  - id: first\n    url: https://github.com/a/b.git\n    claude: &flag no\n    selection: {mode: explicit, include: [{source: skill, destination: first}]}\n  - id: second\n    url: https://github.com/a/b.git\n    claude: *flag\n    selection: {mode: explicit, include: [{source: skill, destination: second}]}\n").unwrap();
    assert!(
        load_manifest(root.path())
            .unwrap()
            .dependencies
            .iter()
            .all(|dep| !dep.claude)
    );

    Ok(())
}

#[test]
fn overlay_cannot_extend_discovery_escape_sparse_or_reuse_routed_destination() -> Result<()> {
    let extension =
        json!({"version":1,"extend":[{"id":"example","include":[{"source":"skills/beta","destination":"beta"}]}]});
    let mut discovery = dependency();
    discovery["selection"] = json!({"mode":"discovery","root":"."});
    let root = fixture(json!([discovery]), Some(extension.clone()))?;
    assert!(
        load_manifest(root.path())
            .unwrap_err()
            .to_string()
            .contains("cannot extend discovery")
    );
    let mut sparse = dependency();
    sparse["sparse"] = json!(["skills/alpha"]);
    let root = fixture(json!([sparse]), Some(extension))?;
    assert!(
        load_manifest(root.path())
            .unwrap_err()
            .to_string()
            .contains("outside example sparse paths")
    );
    let mut routed = dependency();
    routed["router"] = json!({"name":"vendor","description":"Vendor"});
    let extension =
        json!({"version":1,"extend":[{"id":"example","include":[{"source":"skills/beta","destination":"alpha"}]}]});
    let root = fixture(json!([routed.clone()]), Some(extension))?;
    assert!(
        load_manifest(root.path())
            .unwrap_err()
            .to_string()
            .contains("duplicate destination")
    );
    let root = fixture(json!([routed]), None)?;
    fs::create_dir(root.path().join("vendor")).unwrap();
    fs::write(root.path().join("vendor/SKILL.md"), "authored").unwrap();
    assert!(
        load_manifest(root.path())
            .unwrap_err()
            .to_string()
            .contains("authored skill")
    );

    Ok(())
}

#[test]
fn overlay_cannot_remove_added_dependency_or_accept_file_url() -> Result<()> {
    let mut local = dependency();
    local["id"] = json!("local");
    let root = fixture(
        json!([]),
        Some(json!({"version":1,"add":[local.clone()],"remove":["local"]})),
    )?;
    assert!(
        load_manifest(root.path())
            .unwrap_err()
            .to_string()
            .contains("unknown base dependency")
    );
    local["url"] = json!("file:///tmp/local");
    let root = fixture(json!([]), Some(json!({"version":1,"add":[local]})))?;
    assert!(
        load_manifest(root.path())
            .unwrap_err()
            .to_string()
            .contains("HTTPS or SSH")
    );

    Ok(())
}
