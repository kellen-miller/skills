use anyhow::{Context, Result, bail};
use fs2::FileExt;
use serde_json::{Value, json};
use std::{
    fs::{self, File, Metadata},
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

const SOURCE_URL: &str = "https://github.com/example/source.git";

struct Fixture {
    _temporary: TempDir,
    root: PathBuf,
    source: PathBuf,
    cache: PathBuf,
    target: PathBuf,
    dependency: Value,
}

impl Fixture {
    fn new() -> Result<Self> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path().join("skills");
        let source = temporary.path().join("source");
        let cache = temporary.path().join("cache");
        let target = temporary.path().join("claude");
        fs::create_dir_all(&root)?;
        write(
            source.join("skills/alpha/SKILL.md"),
            "---\nname: alpha\n---\n",
        )?;
        let fixture = Self {
            _temporary: temporary,
            root,
            source,
            cache,
            target,
            dependency: json!({
                "id": "example", "url": SOURCE_URL, "ref": "HEAD", "claude": true,
                "selection": {"mode": "explicit", "include": [
                    {"source": "skills/alpha", "destination": "alpha"}
                ]}
            }),
        };
        fixture.git(&["init", "-b", "main"])?;
        fixture.git(&["config", "user.name", "Test User"])?;
        fixture.git(&["config", "user.email", "test@example.com"])?;
        fixture.git(&["config", "commit.gpgsign", "false"])?;
        fixture.commit()?;
        fixture.manifest()?;
        Ok(fixture)
    }

    fn manifest(&self) -> Result<()> {
        write(
            self.root.join("deps.yaml"),
            serde_json::to_string(&json!({"version": 1, "dependencies": [&self.dependency]}))?,
        )
    }

    fn git(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.source)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()?;
        success(&output);
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    }

    fn commit(&self) -> Result<()> {
        self.git(&["add", "-A"])?;
        self.git(&["commit", "-qm", "Update fixture"])?;
        Ok(())
    }

    fn run(&self, command: &str, flags: &[&str]) -> Result<Output> {
        let mut process = Command::new(env!("CARGO_BIN_EXE_skillctl"));
        process
            .arg(command)
            .arg("--root")
            .arg(&self.root)
            .args(flags)
            .env("XDG_CACHE_HOME", &self.cache)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_COUNT", "1")
            .env(
                "GIT_CONFIG_KEY_0",
                format!("url.file://{}.insteadOf", self.source.display()),
            )
            .env("GIT_CONFIG_VALUE_0", SOURCE_URL)
            .env("GIT_TERMINAL_PROMPT", "0");
        if command == "sync" {
            process.arg("--no-claude");
        } else {
            process.arg("--claude-target").arg(&self.target);
        }

        Ok(process.output()?)
    }

    fn sync(&self) -> Result<String> {
        let output = self.run("sync", &[])?;
        success(&output);
        Ok(String::from_utf8(output.stdout)?)
    }

    fn claude(&self, flags: &[&str]) -> Result<String> {
        let output = self.run("install-claude", flags)?;
        success(&output);
        Ok(String::from_utf8(output.stdout)?)
    }

    fn installed(&self) -> PathBuf {
        self.root.join("_managed/alpha/SKILL.md")
    }

    fn lock_path(&self) -> PathBuf {
        self.root.join(".skillctl.lock")
    }

    fn lock(&self) -> Result<Value> {
        Ok(serde_json::from_slice(&fs::read(self.lock_path())?)?)
    }
}

fn write(path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> Result<()> {
    let path = path.as_ref();
    fs::create_dir_all(path.parent().context("fixture file has no parent")?)?;
    fs::write(path, content)?;
    Ok(())
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "status: {}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn absent(path: impl AsRef<Path>) -> Result<()> {
    match fs::symlink_metadata(path.as_ref()) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => bail!("{} unexpectedly exists", path.as_ref().display()),
        Err(error) => Err(error.into()),
    }
}

fn unchanged(before: &Metadata, path: impl AsRef<Path>) -> Result<()> {
    let after = fs::metadata(path)?;
    assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
    assert_eq!(before.modified()?, after.modified()?);
    Ok(())
}

#[test]
fn warm_sync_preserves_files_and_locked_inode() -> Result<()> {
    let f = Fixture::new()?;
    f.sync()?;
    let installed = fs::metadata(f.installed()).unwrap();
    let lock = fs::metadata(f.lock_path()).unwrap();
    let output = f.sync()?;
    unchanged(&installed, f.installed())?;
    unchanged(&lock, f.lock_path())?;
    for action in ["ADD ", "UPDATE ", "REMOVE "] {
        assert!(!output.contains(action), "{output}");
    }

    let record = f.lock()?;
    assert_eq!(record["version"], 1);
    assert_eq!(record["dependencies"].as_array().unwrap().len(), 1);
    assert_eq!(record["dependencies"][0]["id"], "example");
    assert_eq!(record["dependencies"][0]["url"], SOURCE_URL);
    assert_eq!(record["dependencies"][0]["ref"], "HEAD");
    assert_eq!(
        record["dependencies"][0]["commit"],
        f.git(&["rev-parse", "HEAD"])?
    );
    assert!(record["state"]["inputs"].is_object());
    assert!(!record["state"]["output"].as_str().unwrap().is_empty());
    absent(f.root.join(".skillctl-state.json"))?;

    Ok(())
}

#[test]
fn unrelated_commit_advances_resolution_without_replacing_output() -> Result<()> {
    let f = Fixture::new()?;
    f.sync()?;
    let before = fs::metadata(f.installed()).unwrap();
    let old = f.lock()?;
    write(f.source.join("README.md"), "Unrelated change")?;
    f.commit()?;
    f.sync()?;
    unchanged(&before, f.installed())?;
    let next = f.lock()?;
    assert_ne!(
        old["dependencies"][0]["commit"],
        next["dependencies"][0]["commit"]
    );
    assert_eq!(
        next["dependencies"][0]["commit"],
        f.git(&["rev-parse", "HEAD"])?
    );
    assert_eq!(old["state"], next["state"]);

    Ok(())
}

#[test]
fn selected_file_changes_and_deletions_publish_together() -> Result<()> {
    let f = Fixture::new()?;
    write(f.source.join("skills/alpha/obsolete.md"), "Obsolete")?;
    f.commit()?;
    f.sync()?;
    write(
        f.source.join("skills/alpha/SKILL.md"),
        "Updated upstream skill\n",
    )?;
    fs::remove_file(f.source.join("skills/alpha/obsolete.md")).unwrap();
    f.commit()?;
    assert!(f.sync()?.contains("UPDATE alpha"));
    assert!(
        fs::read_to_string(f.installed())
            .unwrap()
            .contains("Updated upstream skill")
    );
    absent(f.root.join("_managed/alpha/obsolete.md"))?;

    Ok(())
}

#[test]
fn destination_rename_and_router_replace_previous_layout() -> Result<()> {
    let mut f = Fixture::new()?;
    f.sync()?;
    f.dependency["selection"]["include"][0]["destination"] = json!("renamed");
    f.manifest()?;
    f.sync()?;
    absent(f.root.join("_managed/alpha"))?;
    assert!(
        fs::read_to_string(f.root.join("_managed/renamed/SKILL.md"))
            .unwrap()
            .contains("name: renamed")
    );
    f.dependency["router"] = json!({"name": "vendor", "description": "Vendor skills"});
    f.manifest()?;
    f.sync()?;
    absent(f.root.join("_managed/renamed"))?;
    assert!(
        f.root
            .join("_managed/vendor/skills/renamed/GUIDE.md")
            .is_file()
    );
    assert!(f.root.join("_managed/vendor/SKILL.md").is_file());

    Ok(())
}

#[test]
fn discovery_follows_renames_and_dependency_removal() -> Result<()> {
    let mut f = Fixture::new()?;
    f.dependency["selection"] = json!({"mode": "discovery", "root": "skills"});
    f.manifest()?;
    f.sync()?;
    fs::rename(f.source.join("skills/alpha"), f.source.join("skills/beta")).unwrap();
    f.commit()?;
    f.sync()?;
    absent(f.root.join("_managed/alpha"))?;
    assert!(f.root.join("_managed/beta/SKILL.md").is_file());
    write(f.root.join("deps.yaml"), "version: 1\ndependencies: []\n")?;
    f.sync()?;
    assert_eq!(fs::read_dir(f.root.join("_managed")).unwrap().count(), 0);
    assert!(f.lock()?["dependencies"].as_array().unwrap().is_empty());

    Ok(())
}

#[test]
fn sync_repairs_local_edits_missing_files_modes_and_stray_git() -> Result<()> {
    let f = Fixture::new()?;
    let script = f.source.join("skills/alpha/run.sh");
    write(&script, "#!/bin/sh\nexit 0\n")?;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    f.commit()?;
    f.sync()?;
    write(f.installed(), "Local edit")?;
    write(
        f.root.join("_managed/obsolete/extra.md"),
        "Unexpected directory",
    )?;
    write(
        f.root.join("_managed/alpha/.git/config"),
        "Unexpected metadata",
    )?;
    let installed_script = f.root.join("_managed/alpha/run.sh");
    fs::set_permissions(&installed_script, fs::Permissions::from_mode(0o644)).unwrap();
    f.sync()?;
    assert!(
        fs::read_to_string(f.installed())
            .unwrap()
            .contains("name: alpha")
    );
    absent(f.root.join("_managed/obsolete"))?;
    absent(f.root.join("_managed/alpha/.git"))?;
    assert_ne!(
        fs::metadata(installed_script).unwrap().permissions().mode() & 0o111,
        0
    );
    fs::remove_file(f.installed()).unwrap();
    f.sync()?;
    assert!(f.installed().is_file());

    Ok(())
}

#[test]
fn missing_and_corrupt_git_cache_are_rebuilt() -> Result<()> {
    let f = Fixture::new()?;
    f.sync()?;
    let before = fs::metadata(f.installed()).unwrap();
    fs::remove_dir_all(f.cache.join("skillctl")).unwrap();
    f.sync()?;
    let clones: Vec<_> = fs::read_dir(f.cache.join("skillctl/repos"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert!(!clones.is_empty());
    for clone in clones {
        fs::remove_dir_all(clone.join(".git")).unwrap();
    }

    f.sync()?;
    unchanged(&before, f.installed())?;

    Ok(())
}

#[test]
fn malformed_empty_and_trailing_lock_records_are_repaired_in_place() -> Result<()> {
    let f = Fixture::new()?;
    f.sync()?;
    let before = fs::metadata(f.installed()).unwrap();
    let original = fs::read(f.lock_path()).unwrap();
    let inode = fs::metadata(f.lock_path()).unwrap().ino();
    for contents in [
        Vec::new(),
        b"broken".to_vec(),
        [original.as_slice(), b"\ntrailing garbage"].concat(),
        [original.as_slice(), b"\n{}"].concat(),
    ] {
        write(f.lock_path(), contents)?;
        f.sync()?;
        assert_eq!(f.lock()?["version"], 1);
        assert_eq!(fs::metadata(f.lock_path()).unwrap().ino(), inode);
        unchanged(&before, f.installed())?;
    }

    Ok(())
}

#[test]
fn dry_run_and_failed_fetch_preserve_live_state() -> Result<()> {
    let f = Fixture::new()?;
    f.sync()?;
    let before = fs::metadata(f.installed()).unwrap();
    let lock = fs::read(f.lock_path()).unwrap();
    write(f.source.join("skills/alpha/SKILL.md"), "Changed upstream\n")?;
    f.commit()?;
    let output = f.run("sync", &["--dry-run"])?;
    success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("UPDATE alpha"));
    unchanged(&before, f.installed())?;
    assert_eq!(lock, fs::read(f.lock_path()).unwrap());
    fs::rename(&f.source, f.source.with_extension("offline")).unwrap();
    assert!(!f.run("sync", &[])?.status.success());
    unchanged(&before, f.installed())?;
    assert_eq!(lock, fs::read(f.lock_path()).unwrap());

    Ok(())
}

#[test]
fn invalid_ref_and_missing_source_preserve_live_state() -> Result<()> {
    let mut f = Fixture::new()?;
    f.sync()?;
    let before = fs::metadata(f.installed()).unwrap();
    let lock = fs::read(f.lock_path()).unwrap();
    f.dependency["ref"] = json!("does-not-exist");
    f.manifest()?;
    let output = f.run("sync", &[])?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("acquisition failed"));
    unchanged(&before, f.installed())?;
    assert_eq!(lock, fs::read(f.lock_path()).unwrap());
    f.dependency["ref"] = json!("HEAD");
    f.manifest()?;
    fs::remove_dir_all(f.source.join("skills/alpha")).unwrap();
    write(f.source.join("README.md"), "Skill removed")?;
    f.commit()?;
    let output = f.run("sync", &[])?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("source is not a skill"));
    unchanged(&before, f.installed())?;
    assert_eq!(lock, fs::read(f.lock_path()).unwrap());

    Ok(())
}

#[test]
fn pinned_commit_stays_fixed_and_branch_follows_force_update() -> Result<()> {
    let mut f = Fixture::new()?;
    let pinned = f.git(&["rev-parse", "HEAD"])?;
    f.dependency["ref"] = json!(pinned);
    f.manifest()?;
    f.sync()?;
    write(
        f.source.join("skills/alpha/SKILL.md"),
        "New branch contents\n",
    )?;
    f.commit()?;
    f.sync()?;
    assert!(
        !fs::read_to_string(f.installed())
            .unwrap()
            .contains("New branch contents")
    );
    assert_eq!(f.lock()?["dependencies"][0]["commit"], pinned);
    f.dependency["ref"] = json!("main");
    f.manifest()?;
    f.sync()?;
    write(
        f.source.join("skills/alpha/SKILL.md"),
        "Rewritten history\n",
    )?;
    f.git(&["add", "-A"])?;
    f.git(&["commit", "--amend", "--no-edit", "-q"])?;
    f.sync()?;
    assert!(
        fs::read_to_string(f.installed())
            .unwrap()
            .contains("Rewritten history")
    );

    Ok(())
}

#[test]
fn head_follows_remote_default_branch_change() -> Result<()> {
    let f = Fixture::new()?;
    f.sync()?;
    f.git(&["checkout", "-qb", "replacement"])?;
    write(
        f.source.join("skills/alpha/SKILL.md"),
        "New default branch\n",
    )?;
    f.commit()?;
    f.sync()?;
    assert!(
        fs::read_to_string(f.installed())
            .unwrap()
            .contains("New default branch")
    );

    Ok(())
}

#[test]
fn sparse_selection_change_uses_new_contents() -> Result<()> {
    let mut f = Fixture::new()?;
    write(
        f.source.join("other/beta/SKILL.md"),
        "---\nname: beta\n---\nOther selection\n",
    )?;
    f.commit()?;
    f.dependency["sparse"] = json!(["skills"]);
    f.manifest()?;
    f.sync()?;
    f.dependency["sparse"] = json!(["other"]);
    f.dependency["selection"]["include"] = json!([{"source": "other/beta", "destination": "beta"}]);
    f.manifest()?;
    f.sync()?;
    absent(f.root.join("_managed/alpha"))?;
    assert!(
        fs::read_to_string(f.root.join("_managed/beta/SKILL.md"))
            .unwrap()
            .contains("Other selection")
    );

    Ok(())
}

#[test]
fn safe_symlink_targets_outside_selection_and_aliased_ancestors_refresh() -> Result<()> {
    let mut f = Fixture::new()?;
    write(f.source.join("shared.md"), "Original reference")?;
    symlink(
        "../../shared.md",
        f.source.join("skills/alpha/reference.md"),
    )
    .unwrap();
    symlink("skills", f.source.join("alias")).unwrap();
    f.commit()?;
    f.dependency["selection"]["include"][0]["source"] = json!("alias/alpha");
    f.manifest()?;
    f.sync()?;
    write(f.source.join("shared.md"), "Updated external reference")?;
    f.commit()?;
    f.sync()?;
    let reference = f.root.join("_managed/alpha/reference.md");
    assert_eq!(
        fs::read_to_string(&reference).unwrap(),
        "Updated external reference"
    );
    assert!(
        !fs::symlink_metadata(reference)
            .unwrap()
            .file_type()
            .is_symlink()
    );

    Ok(())
}

#[test]
fn escaping_and_cyclic_symlinks_fail_without_publishing() -> Result<()> {
    for cycle in [false, true] {
        let f = Fixture::new()?;
        f.sync()?;
        let before = fs::metadata(f.installed()).unwrap();
        let lock = fs::read(f.lock_path()).unwrap();
        let outside = f._temporary.path().join("outside.md");
        write(&outside, "Outside repository")?;
        let target = if cycle {
            Path::new(".")
        } else {
            outside.as_path()
        };
        symlink(target, f.source.join("skills/alpha/unsafe")).unwrap();
        f.commit()?;
        let output = f.run("sync", &[])?;
        assert!(!output.status.success(), "cycle: {cycle}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains(if cycle {
                "symlink cycle"
            } else {
                "symlink escapes repository"
            }),
            "{error}"
        );
        unchanged(&before, f.installed())?;
        assert_eq!(lock, fs::read(f.lock_path()).unwrap());
    }

    Ok(())
}

#[test]
fn router_guide_collision_preserves_previous_output() -> Result<()> {
    let mut f = Fixture::new()?;
    f.sync()?;
    let before = fs::metadata(f.installed()).unwrap();
    let lock = fs::read(f.lock_path()).unwrap();
    write(f.source.join("skills/alpha/GUIDE.md"), "Existing reference")?;
    f.commit()?;
    f.dependency["router"] = json!({"name": "vendor", "description": "Vendor skills"});
    f.manifest()?;
    let output = f.run("sync", &[])?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("GUIDE.md"));
    unchanged(&before, f.installed())?;
    assert_eq!(lock, fs::read(f.lock_path()).unwrap());
    absent(f.root.join("_managed/vendor"))?;

    Ok(())
}

#[test]
fn root_and_shared_cache_locks_exclude_cli_processes() -> Result<()> {
    let f = Fixture::new()?;
    f.sync()?;
    let before = fs::metadata(f.installed()).unwrap();
    let lock_contents = fs::read(f.lock_path()).unwrap();
    for path in [f.lock_path(), f.cache.join("skillctl/.skillctl.lock")] {
        let held = File::open(&path).unwrap();
        FileExt::lock_exclusive(&held).unwrap();
        let output = f.run("sync", &[])?;
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("another sync is in progress"));
        unchanged(&before, f.installed())?;
        assert_eq!(lock_contents, fs::read(f.lock_path()).unwrap());
        FileExt::unlock(&held).unwrap();
    }

    f.sync()?;

    Ok(())
}

#[test]
fn claude_preserves_unchanged_files_lock_and_unowned_entries() -> Result<()> {
    let f = Fixture::new()?;
    f.sync()?;
    write(
        f.root.join("authored/SKILL.md"),
        "---\nname: authored\n---\n",
    )?;
    let lock = fs::read(f.lock_path()).unwrap();
    let lock_info = fs::metadata(f.lock_path()).unwrap();
    f.claude(&[])?;
    let authored = f.target.join("authored/SKILL.md");
    let before = fs::metadata(&authored).unwrap();
    let alpha = fs::metadata(f.target.join("alpha/SKILL.md")).unwrap();
    f.claude(&[])?;
    unchanged(&before, &authored)?;
    unchanged(&alpha, f.target.join("alpha/SKILL.md"))?;
    unchanged(&lock_info, f.lock_path())?;
    assert_eq!(lock, fs::read(f.lock_path()).unwrap());
    write(f.target.join("unowned/keep.md"), "Keep")?;
    write(f.root.join("authored/extra.md"), "New reference")?;
    fs::remove_dir_all(f.root.join("_managed/alpha")).unwrap();
    f.claude(&[])?;
    assert!(f.target.join("authored/extra.md").is_file());
    assert_eq!(
        fs::read_to_string(f.target.join("unowned/keep.md")).unwrap(),
        "Keep"
    );
    absent(f.target.join("alpha"))?;
    assert_eq!(lock, fs::read(f.lock_path()).unwrap());

    Ok(())
}

#[test]
fn claude_refuses_to_overwrite_unowned_skill() -> Result<()> {
    let f = Fixture::new()?;
    f.sync()?;
    let path = f.target.join("alpha/SKILL.md");
    write(&path, "Mine")?;
    let output = f.run("install-claude", &[])?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not managed by skillctl"));
    assert_eq!(fs::read_to_string(path).unwrap(), "Mine");

    Ok(())
}

#[test]
fn claude_dry_run_preserves_files_and_hidden_dependency_removes_owned_skill() -> Result<()> {
    let mut f = Fixture::new()?;
    f.sync()?;
    f.claude(&["--dry-run"])?;
    absent(f.target.join("alpha"))?;
    absent(f.target.join(".skillctl-owned"))?;
    f.claude(&[])?;
    let installed = f.target.join("alpha/SKILL.md");
    let before = fs::metadata(&installed).unwrap();
    let ownership = fs::read(f.target.join(".skillctl-owned")).unwrap();
    f.dependency["claude"] = json!(false);
    f.manifest()?;
    assert!(f.claude(&["--dry-run"])?.contains("REMOVE alpha"));
    unchanged(&before, &installed)?;
    assert_eq!(
        ownership,
        fs::read(f.target.join(".skillctl-owned")).unwrap()
    );
    f.claude(&[])?;
    absent(f.target.join("alpha"))?;

    Ok(())
}
