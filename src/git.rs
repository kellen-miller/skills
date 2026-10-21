use crate::manifest::Dependency;
use anyhow::{Context, Result, bail};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) fn run_git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .context("cannot start Git")?;
    if !output.status.success() {
        bail!(
            "git {}: {}",
            args.first().unwrap_or(&"command"),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

pub(crate) fn cache_root() -> Result<PathBuf> {
    if let Some(base) = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return Ok(base.join("skillctl"));
    }

    let home = env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".cache/skillctl"))
}

pub(crate) fn acquire_dependency(dependency: &Dependency, cache: &Path) -> Result<PathBuf> {
    let identity = serde_json::to_vec(&json!([
        dependency.id,
        dependency.url,
        dependency.reference,
        dependency.sparse
    ]))?;
    let clone = cache
        .join("repos")
        .join(format!("{:x}", Sha256::digest(identity)));
    if clone.exists() {
        let valid = (|| -> Result<bool> {
            let git_dir = run_git(&clone, &["rev-parse", "--absolute-git-dir"])?;
            let origin = run_git(&clone, &["config", "--get", "remote.origin.url"])?;
            Ok(
                fs::canonicalize(git_dir)? == fs::canonicalize(clone.join(".git"))?
                    && origin == dependency.url,
            )
        })()
        .unwrap_or(false);
        if !valid {
            fs::remove_dir_all(&clone)?;
        }
    }

    if !clone.exists() {
        fs::create_dir_all(&clone)?;
        run_git(&clone, &["init", "--quiet"])?;
        run_git(&clone, &["remote", "add", "origin", &dependency.url])?;
    }

    let result = (|| -> Result<()> {
        let reference = if dependency.reference.is_empty() {
            "HEAD"
        } else {
            &dependency.reference
        };
        run_git(
            &clone,
            &[
                "fetch",
                "--depth",
                "1",
                "--filter=blob:none",
                "--no-tags",
                "--force",
                "origin",
                reference,
            ],
        )?;
        if !dependency.sparse.is_empty() {
            let mut args = vec!["sparse-checkout", "set"];
            args.extend(dependency.sparse.iter().map(String::as_str));
            run_git(&clone, &args)?;
        }

        run_git(&clone, &["checkout", "--force", "--detach", "FETCH_HEAD"])?;
        run_git(&clone, &["clean", "-ffdx"])?;
        Ok(())
    })();
    if let Err(error) = result {
        // Failed refreshes never fall back to stale content.
        if let Err(cleanup_error) = fs::remove_dir_all(&clone) {
            return Err(error).context(format!(
                "{} acquisition failed; cannot remove incomplete cache: {cleanup_error}",
                dependency.id
            ));
        }

        return Err(error).with_context(|| format!("{} acquisition failed", dependency.id));
    }

    Ok(clone)
}
