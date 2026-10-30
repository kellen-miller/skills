use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub(crate) fn tree_fingerprint(root: &Path) -> Result<String> {
    match fs::symlink_metadata(root) {
        Ok(info) if info.is_dir() => (),
        Ok(_) => return Ok(String::new()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(error) => return Err(error.into()),
    }

    let mut digest = Sha256::new();
    for entry in WalkDir::new(root)
        .min_depth(1)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| entry.file_name() != "target")
    {
        let entry = entry?;
        let path = entry.path();
        let info = fs::symlink_metadata(path)?;
        let (kind, content) = if info.file_type().is_symlink() {
            (
                "link",
                fs::read_link(path)?.as_os_str().as_encoded_bytes().to_vec(),
            )
        } else if info.is_file() {
            ("file", fs::read(path)?)
        } else if info.is_dir() {
            ("directory", Vec::new())
        } else {
            bail!("unsupported skill file: {}", path.display());
        };
        let relative = path.strip_prefix(root)?.to_string_lossy();
        #[cfg(unix)]
        let mode = info.permissions().mode();
        #[cfg(not(unix))]
        let mode = if info.permissions().readonly() {
            0o444
        } else {
            0o644
        };
        digest.update(serde_json::to_vec(&(relative, kind, mode, content.len()))?);
        digest.update(b"\n");
        digest.update(content);
    }

    Ok(hex::encode(digest.finalize()))
}

pub(crate) fn trees_equal(left: &Path, right: &Path) -> Result<bool> {
    let left = tree_fingerprint(left)?;
    Ok(!left.is_empty() && left == tree_fingerprint(right)?)
}

pub(crate) fn directory_names(root: &Path) -> Result<Vec<String>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut names = entries
        .map(|entry| -> Result<String> {
            entry?
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("non-UTF-8 skill directory name"))
        })
        .collect::<Result<Vec<_>>>()?;
    names.sort();
    Ok(names)
}

pub(crate) fn copy_skill_tree(
    source: &Path,
    target: &Path,
    repository: Option<&Path>,
) -> Result<()> {
    let repository = repository.map(fs::canonicalize).transpose()?;
    copy_entry(source, target, repository.as_deref(), &mut HashSet::new())
}

fn copy_entry(
    source: &Path,
    target: &Path,
    repository: Option<&Path>,
    ancestors: &mut HashSet<PathBuf>,
) -> Result<()> {
    let resolved =
        fs::canonicalize(source).with_context(|| format!("cannot resolve {}", source.display()))?;
    if let Some(repository) = repository {
        ensure!(
            resolved.starts_with(repository),
            "source symlink escapes repository: {}",
            source.display()
        );
    }

    let info = fs::metadata(&resolved)?;
    if info.is_dir() {
        ensure!(
            ancestors.insert(resolved.clone()),
            "source symlink cycle: {}",
            source.display()
        );
        fs::create_dir(target)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            if entry.file_name() != ".git" && entry.file_name() != "target" {
                copy_entry(
                    &entry.path(),
                    &target.join(entry.file_name()),
                    repository,
                    ancestors,
                )?;
            }
        }

        ancestors.remove(&resolved);
        fs::set_permissions(target, info.permissions())?;
        return Ok(());
    }

    ensure!(
        info.is_file(),
        "unsupported skill file: {}",
        source.display()
    );
    fs::copy(source, target)?;
    fs::set_permissions(target, info.permissions())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{copy_skill_tree, tree_fingerprint};
    use anyhow::Result;
    use std::fs;

    #[test]
    fn build_artifacts_do_not_change_or_enter_installed_skills() -> Result<()> {
        let fixture = tempfile::tempdir()?;
        let source = fixture.path().join("skill");
        fs::create_dir(&source)?;
        fs::write(source.join("SKILL.md"), "instructions")?;
        let before = tree_fingerprint(&source)?;
        fs::create_dir_all(source.join("target/debug"))?;
        fs::write(source.join("target/debug/binary"), "build artifact")?;
        assert_eq!(before, tree_fingerprint(&source)?);
        let installed = fixture.path().join("installed");
        copy_skill_tree(&source, &installed, None)?;
        assert!(!installed.join("target").exists());
        assert!(installed.join("SKILL.md").is_file());
        Ok(())
    }
}
