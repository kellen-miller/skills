use anyhow::Result;
use std::{fs, process::Command};

#[test]
fn package_contains_tracked_skill_resources_and_refuses_existing_output() -> Result<()> {
    let checkout = tempfile::tempdir()?;
    let root = checkout.path();
    fs::write(root.join("deps.yaml"), "version: 1\ndependencies: []\n")?;
    fs::write(root.join("LICENSE"), "MIT\n")?;
    fs::create_dir_all(root.join("example/scripts"))?;
    fs::write(
        root.join("example/SKILL.md"),
        "---\nname: example\ndescription: Example\n---\n",
    )?;
    fs::write(root.join("example/scripts/run"), "bundled resource")?;
    for args in [vec!["init", "--quiet"], vec!["add", "example"]] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(root)
                .status()?
                .success()
        );
    }

    fs::create_dir_all(root.join("example/target"))?;
    fs::write(root.join("example/target/private"), "must not ship")?;
    let result = Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .args(["package-plugin", "--root"])
        .arg(root)
        .output()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let plugin = root.join("dist/kellen-skills");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(plugin.join("plugin.json"))?)?;
    assert_eq!(manifest["name"], "kellen-skills");
    assert_eq!(
        fs::read_to_string(plugin.join("skills/example/scripts/run"))?,
        "bundled resource"
    );
    assert!(!plugin.join("skills/example/target").exists());
    assert!(plugin.join("LICENSE").is_file());
    let result = Command::new(env!("CARGO_BIN_EXE_skillctl"))
        .args(["package-plugin", "--root"])
        .arg(root)
        .output()?;
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("already exists"));
    assert!(plugin.join("skills/example/SKILL.md").is_file());
    Ok(())
}

#[cfg(unix)]
#[test]
fn launcher_scopes_build_environment_and_preserves_execution_context() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let fixture = tempfile::tempdir()?;
    let root = fixture.path();
    let skill = root.join("copied skill");
    fs::create_dir_all(skill.join("scripts"))?;
    fs::write(
        skill.join("scripts/run"),
        include_str!("../dispatch-tickets/scripts/run"),
    )?;
    fs::write(
        skill.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"build-toolchain\"\n",
    )?;
    let tools = root.join("bin");
    fs::create_dir(&tools)?;
    let compiler = tools.join("rustc");
    fs::write(&compiler, "#!/bin/sh\necho 'host: test-host'\n")?;
    fs::set_permissions(&compiler, fs::Permissions::from_mode(0o755))?;
    let cargo = tools.join("cargo");
    fs::write(
        &cargo,
        r#"#!/bin/sh
set -eu
[ "$RUSTUP_TOOLCHAIN" = "build-toolchain" ]
[ "$PWD" = "$EXPECTED_BUILD_DIR" ]
while [ "$1" != "--target-dir" ]; do shift; done
shift
mkdir -p "$1/test-host/debug"
cat > "$1/test-host/debug/dispatch-tickets" <<'WORKER'
#!/bin/sh
printf '%s\n' "$RUSTUP_TOOLCHAIN" "$PWD" "$@"
exit 23
WORKER
chmod +x "$1/test-host/debug/dispatch-tickets"
"#,
    )?;
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755))?;
    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![tools];
    paths.extend(std::env::split_paths(&current_path));
    let result = Command::new("sh")
        .arg(skill.join("scripts/run"))
        .arg("argument with spaces")
        .env("PATH", std::env::join_paths(paths)?)
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("EXPECTED_BUILD_DIR", fs::canonicalize(&skill)?)
        .env("RUSTUP_TOOLCHAIN", "consumer-toolchain")
        .env("CARGO_BUILD_TARGET", "unavailable-cross-target")
        .current_dir(root)
        .output()?;
    assert_eq!(
        result.status.code(),
        Some(23),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = String::from_utf8(result.stdout)?;
    let expected = fs::canonicalize(root)?;
    assert_eq!(
        output.lines().collect::<Vec<_>>(),
        vec![
            "consumer-toolchain",
            expected.to_str().unwrap(),
            "argument with spaces"
        ]
    );
    assert!(!skill.join("target").exists());
    Ok(())
}
