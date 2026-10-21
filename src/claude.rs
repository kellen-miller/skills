use crate::{
    generation::set_skill_name,
    lock::sync_lock,
    manifest::{Manifest, top_level_names},
    tree::{copy_skill_tree, directory_names, trees_equal},
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::Path,
};

struct ClaudePlan {
    stage: tempfile::TempDir,
    sources: BTreeMap<String, std::path::PathBuf>,
    owned: BTreeSet<String>,
    names: BTreeSet<String>,
    changed: Vec<String>,
    removed: Vec<String>,
}

pub(crate) fn install_claude(
    root: &Path,
    manifest: &Manifest,
    target: &Path,
    dry_run: bool,
    out: &mut dyn Write,
) -> Result<()> {
    let _lock = sync_lock(root)?;
    let plan = stage_claude_install(root, manifest, target)?;
    let owned_path = target.join(".skillctl-owned");
    for name in plan.changed.iter().filter(|name| plan.owned.contains(*name)) {
        writeln!(out, "UPDATE {name}")?;
    }
    for name in plan.sources.keys().filter(|name| !plan.owned.contains(*name)) {
        writeln!(out, "ADD {name}")?;
    }
    for name in &plan.removed {
        writeln!(out, "REMOVE {name}")?;
    }

    if dry_run {
        writeln!(
            out,
            "Would install {} skills into {}",
            plan.sources.len(),
            target.display()
        )?;
        return Ok(());
    }

    let union: Vec<_> = plan.owned.union(&plan.names).cloned().collect();
    fs::write(&owned_path, union.join("\n") + "\n")?;
    for name in &plan.removed {
        remove_owned(&target.join(name))?;
    }

    for name in plan.changed {
        let destination = target.join(&name);
        remove_owned(&destination)?;
        fs::rename(plan.stage.path().join(&name), destination)
            .with_context(|| format!("cannot install Claude skill {name}"))?;
    }

    fs::write(
        &owned_path,
        plan.names.into_iter().collect::<Vec<_>>().join("\n") + "\n",
    )?;
    writeln!(out, "Installed {} skills into {}", plan.sources.len(), target.display())?;
    Ok(())
}

fn stage_claude_install(root: &Path, manifest: &Manifest, target: &Path) -> Result<ClaudePlan> {
    let hidden: BTreeSet<_> = manifest
        .dependencies
        .iter()
        .filter(|dependency| !dependency.claude)
        .flat_map(top_level_names)
        .collect();
    let mut sources: BTreeMap<_, _> = manifest
        .durable_roots
        .iter()
        .map(|name| (name.clone(), root.join(name)))
        .collect();
    let managed = root.join("_managed");
    for name in directory_names(&managed)? {
        let path = managed.join(&name);
        if !hidden.contains(&name) && path.is_dir() && path.join("SKILL.md").is_file() {
            sources.insert(name, path);
        }
    }

    fs::create_dir_all(target)?;
    let owned_path = target.join(".skillctl-owned");
    let owned_content = match fs::read_to_string(&owned_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let owned: BTreeSet<_> = owned_content.split_whitespace().map(str::to_owned).collect();
    for name in &owned {
        ensure!(
            !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0']),
            "unsafe Claude ownership entry: {name}"
        );
    }

    for name in sources.keys() {
        match fs::symlink_metadata(target.join(name)) {
            Ok(_) => ensure!(
                owned.contains(name),
                "{} exists and is not managed by skillctl",
                target.join(name).display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }

    let stage = tempfile::Builder::new().prefix(".skillctl-stage-").tempdir_in(target)?;
    let mut changed = Vec::new();
    for (name, source) in &sources {
        let destination = target.join(name);
        let content = fs::read_to_string(source.join("SKILL.md"))?;
        let has_name = content
            .strip_prefix("---\n")
            .and_then(|body| body.split_once("\n---"))
            .is_some_and(|(header, _)| header.lines().any(|line| line == format!("name: {name}")));
        if owned.contains(name) && has_name && trees_equal(source, &destination)? {
            continue;
        }

        let staged = stage.path().join(name);
        copy_skill_tree(source, &staged, None)?;
        set_skill_name(&staged, name)?;
        if !trees_equal(&staged, &destination)? {
            changed.push(name.clone());
        }
    }

    let removed: Vec<_> = owned
        .iter()
        .filter(|name| !sources.contains_key(*name))
        .cloned()
        .collect();
    let names: BTreeSet<_> = sources.keys().cloned().collect();
    Ok(ClaudePlan {
        stage,
        sources,
        owned,
        names,
        changed,
        removed,
    })
}

fn remove_owned(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(info) if info.is_dir() => fs::remove_dir_all(path)?,
        Ok(_) => fs::remove_file(path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }

    Ok(())
}
