//! Assemble root-level authored skills into an installable plugin.
use crate::manifest::Manifest;
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeSet, fs, io::Write, path::Path, process::Command};

pub(crate) fn package_plugin(root: &Path, manifest: &Manifest, out: &mut dyn Write) -> Result<()> {
    let directory = root.join("dist");
    fs::create_dir_all(&directory)?;
    let destination = directory.join("kellen-skills");
    ensure!(
        !destination.exists(),
        "{} already exists; move it before packaging again",
        destination.display()
    );
    let stage = tempfile::tempdir_in(&directory)?;
    fs::write(
        stage.path().join("plugin.json"),
        include_str!("../templates/plugin.json"),
    )?;
    fs::copy(root.join("LICENSE"), stage.path().join("LICENSE"))?;
    let files = Command::new("git")
        .args(["ls-files", "-z", "--"])
        .args(&manifest.durable_roots)
        .current_dir(root)
        .output()?;
    ensure!(files.status.success(), "cannot list authored plugin files");
    let mut names = BTreeSet::new();
    for relative in files
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let relative = Path::new(std::str::from_utf8(relative)?);
        if relative.file_name().is_some_and(|name| name == "SKILL.md")
            && relative.components().count() == 2
        {
            names.insert(
                relative
                    .parent()
                    .context("skill has no directory")?
                    .to_path_buf(),
            );
        }

        let source = root.join(relative);
        let resolved = fs::canonicalize(&source)
            .with_context(|| format!("cannot resolve {}", source.display()))?;
        ensure!(
            resolved.starts_with(root) && resolved.is_file(),
            "plugin file escapes checkout or is not a file: {}",
            source.display()
        );
        let target = stage.path().join("skills").join(relative);
        fs::create_dir_all(target.parent().context("plugin file has no parent")?)?;
        fs::copy(resolved, target)?;
    }

    ensure!(!names.is_empty(), "no tracked authored skills to package");

    fs::rename(stage.path(), &destination)?;
    writeln!(
        out,
        "Packaged {} authored skills into {}",
        names.len(),
        destination.display()
    )?;
    Ok(())
}
