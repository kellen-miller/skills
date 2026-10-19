use crate::{
    generation::{route_guides, set_skill_name, write_router},
    git::{acquire_dependency, cache_root, run_git},
    lock::{
        DependencyInput, GenerationInputs, GenerationState, ResolvedDependency, SyncRecord,
        render_sync_lock, sync_lock, write_sync_lock,
    },
    manifest::{Manifest, Mapping, Router},
    tree::{copy_skill_tree, directory_names, tree_fingerprint, trees_equal},
};
use anyhow::{Context, Result, anyhow, bail, ensure};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};
use tempfile::Builder;
use walkdir::WalkDir;

struct SelectedSkill {
    source: PathBuf,
    relative: PathBuf,
    repository: PathBuf,
    routed: bool,
}

pub fn sync(root: &Path, manifest: &Manifest, dry_run: bool, out: &mut dyn Write) -> Result<()> {
    let mut lock = sync_lock(root)?;
    let cache = cache_root()?;
    fs::create_dir_all(&cache)?;
    let _cache_lock = sync_lock(&cache)?;
    let mut previous_content = Vec::new();
    lock.read_to_end(&mut previous_content)?;
    // Full-file parsing rejects torn shorter writes with trailing old metadata.
    let previous = serde_json::from_slice::<SyncRecord>(&previous_content)
        .ok()
        .filter(|record| record.version == 1)
        .unwrap_or_default();
    let next = AtomicUsize::new(0);
    let results: Vec<_> = manifest
        .dependencies
        .iter()
        .map(|_| Mutex::new(None))
        .collect();
    thread::scope(|scope| -> Result<()> {
        let mut workers = Vec::new();
        for _ in 0..manifest.dependencies.len().min(4) {
            workers.push(scope.spawn(|| -> Result<()> {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= manifest.dependencies.len() {
                        break;
                    }

                    let result = acquire_dependency(&manifest.dependencies[index], &cache);
                    *results[index]
                        .lock()
                        .map_err(|_| anyhow!("acquisition result lock poisoned"))? = Some(result);
                }

                Ok(())
            }));
        }

        for worker in workers {
            worker
                .join()
                .map_err(|_| anyhow!("Git acquisition worker panicked"))??;
        }

        Ok(())
    })?;
    let clones: Vec<_> = results
        .into_iter()
        .map(|result| {
            result
                .into_inner()
                .map_err(|_| anyhow!("acquisition result lock poisoned"))?
                .context("missing acquisition result")?
        })
        .collect::<Result<_>>()?;
    let temporary = Builder::new()
        .prefix(".skills-sync-")
        .tempdir_in(root.parent().context("skills root has no parent")?)?;
    let stage = temporary.path().join("_managed");
    fs::create_dir(&stage)?;
    let mut generator = Sha256::new();
    for (name, content) in [
        ("sync.rs", include_str!("sync.rs")),
        ("manifest.rs", include_str!("manifest.rs")),
        ("generation.rs", include_str!("generation.rs")),
        ("tree.rs", include_str!("tree.rs")),
        ("git.rs", include_str!("git.rs")),
        ("lock.rs", include_str!("lock.rs")),
        ("claude.rs", include_str!("claude.rs")),
        ("lock.json.j2", include_str!("../templates/lock.json.j2")),
        ("router.md.j2", include_str!("../templates/router.md.j2")),
        ("index.md.j2", include_str!("../templates/index.md.j2")),
    ] {
        generator.update(name.as_bytes());
        generator.update(content.len().to_le_bytes());
        generator.update(content.as_bytes());
    }

    let mut inputs = GenerationInputs {
        generator: format!("skillctl-rust:{:x}", generator.finalize()),
        durable_roots: manifest.durable_roots.clone(),
        dependencies: Vec::new(),
    };
    inputs.durable_roots.sort();
    let durable: BTreeSet<_> = manifest.durable_roots.iter().cloned().collect();
    let mut destinations = BTreeSet::new();
    let mut selected = Vec::new();
    let mut routers: Vec<(Router, Vec<String>)> = Vec::new();
    let mut resolved = Vec::new();
    for (dependency, clone) in manifest.dependencies.iter().zip(clones) {
        let mut mappings = dependency.selection.include.clone();
        if dependency.selection.mode == "discovery" {
            mappings.clear();
            let discovery_root = clone.join(&dependency.selection.root);
            let entries = directory_names(&discovery_root).with_context(|| {
                format!(
                    "{} discovery root does not exist: {}",
                    dependency.id, dependency.selection.root
                )
            })?;
            ensure!(
                discovery_root.is_dir(),
                "{} discovery root does not exist: {}",
                dependency.id,
                dependency.selection.root
            );
            for name in entries {
                let source = discovery_root.join(&name);
                if !dependency.selection.exclude.contains(&name)
                    && source.is_dir()
                    && source.join("SKILL.md").is_file()
                {
                    mappings.push(Mapping {
                        source: source.strip_prefix(&clone)?.to_string_lossy().into_owned(),
                        destination: name,
                    });
                }
            }
        }

        if let Some(router) = &dependency.router {
            ensure!(
                !durable.contains(&router.name) && destinations.insert(router.name.clone()),
                "duplicate destination or authored skill collision: {}",
                router.name
            );
            routers.push((
                router.clone(),
                mappings
                    .iter()
                    .map(|mapping| mapping.destination.clone())
                    .collect(),
            ));
        }

        let mut paths = Vec::new();
        let mut has_symlinks = false;
        let resolved_clone = fs::canonicalize(&clone)?;
        for mapping in mappings {
            let relative = if let Some(router) = &dependency.router {
                PathBuf::from(&router.name)
                    .join("skills")
                    .join(&mapping.destination)
            } else {
                ensure!(
                    !durable.contains(&mapping.destination)
                        && destinations.insert(mapping.destination.clone()),
                    "duplicate destination or authored skill collision: {}",
                    mapping.destination
                );
                PathBuf::from(&mapping.destination)
            };
            let source = clone.join(&mapping.source);
            ensure!(
                source.is_dir() && source.join("SKILL.md").is_file(),
                "{} source is not a skill: {}",
                dependency.id,
                mapping.source
            );
            let resolved_source = fs::canonicalize(&source)?;
            ensure!(
                resolved_source.starts_with(&resolved_clone),
                "source symlink escapes repository: {}",
                source.display()
            );
            for ancestor in source
                .ancestors()
                .take_while(|ancestor| ancestor.starts_with(&clone))
            {
                if fs::symlink_metadata(ancestor)?.file_type().is_symlink() {
                    has_symlinks = true;
                }
            }

            for entry in WalkDir::new(&source) {
                let entry = entry?;
                if entry.file_type().is_symlink() {
                    has_symlinks = true;
                    let resolved = fs::canonicalize(entry.path())?;
                    ensure!(
                        resolved.starts_with(&resolved_clone),
                        "source symlink escapes repository: {}",
                        entry.path().display()
                    );
                }
            }

            selected.push(SelectedSkill {
                source,
                relative,
                repository: clone.clone(),
                routed: dependency.router.is_some(),
            });
            paths.push(mapping.source);
        }

        if dependency.selection.mode == "discovery" {
            paths = vec![dependency.selection.root.clone()];
        }

        if has_symlinks {
            paths = vec![".".to_owned()];
        }

        let mut args = vec!["rev-parse".to_owned(), "HEAD".to_owned()];
        args.extend(paths.iter().map(|path| {
            if path == "." {
                "HEAD^{tree}".to_owned()
            } else {
                format!("HEAD:{path}")
            }
        }));
        let revisions: Vec<_> =
            run_git(&clone, &args.iter().map(String::as_str).collect::<Vec<_>>())?
                .lines()
                .map(str::to_owned)
                .collect();
        inputs.dependencies.push(DependencyInput {
            config: dependency.clone(),
            trees: revisions[1..].to_vec(),
        });
        resolved.push(ResolvedDependency {
            id: dependency.id.clone(),
            url: dependency.url.clone(),
            reference: if dependency.reference.is_empty() {
                "HEAD".to_owned()
            } else {
                dependency.reference.clone()
            },
            commit: revisions[0].clone(),
        });
    }

    let current = root.join("_managed");
    let live_fingerprint = tree_fingerprint(&current)?;
    let state = GenerationState {
        inputs,
        output: live_fingerprint.clone(),
    };
    let mut record = SyncRecord {
        version: 1,
        dependencies: resolved,
        state,
    };
    if !live_fingerprint.is_empty() && previous.state == record.state {
        if !dry_run && previous != record {
            write_sync_lock(&mut lock, &render_sync_lock(&record)?)?;
        }

        writeln!(
            out,
            "{} {} managed skills (unchanged)",
            if dry_run {
                "Would install"
            } else {
                "Installed"
            },
            destinations.len()
        )?;
        return Ok(());
    }

    for skill in selected {
        let target = stage.join(&skill.relative);
        fs::create_dir_all(target.parent().context("skill has no parent")?)?;
        copy_skill_tree(&skill.source, &target, Some(&skill.repository))?;
        set_skill_name(
            &target,
            skill
                .relative
                .file_name()
                .context("skill has no name")?
                .to_str()
                .context("skill name is not UTF-8")?,
        )?;
        if skill.routed {
            route_guides(&target)?;
        }
    }

    for (router, members) in routers {
        write_router(&stage.join(&router.name), &router, &members)?;
    }

    let old_names = directory_names(&current)?;
    let new_names = directory_names(&stage)?;
    for name in &new_names {
        if !old_names.contains(name) {
            writeln!(out, "ADD {name}")?;
        } else if !trees_equal(&current.join(name), &stage.join(name))? {
            writeln!(out, "UPDATE {name}")?;
        }
    }

    for name in &old_names {
        if !new_names.contains(name) {
            writeln!(out, "REMOVE {name}")?;
        }
    }

    if dry_run {
        writeln!(out, "Would install {} managed skills", new_names.len())?;
        return Ok(());
    }

    record.state.output = tree_fingerprint(&stage)?;
    let lock_content = render_sync_lock(&record)?;
    if live_fingerprint != record.state.output {
        let backup = temporary.path().join("previous-managed");
        let moved_old = match fs::symlink_metadata(&current) {
            Ok(_) => {
                fs::rename(&current, &backup)?;
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        if let Err(error) = fs::rename(&stage, &current) {
            if moved_old && let Err(restore) = fs::rename(&backup, &current) {
                let recovery = temporary.keep();
                bail!(
                    "install failed: {error}; restore failed: {restore}; backup retained in {}",
                    recovery.display()
                );
            }

            return Err(error.into());
        }
    }

    write_sync_lock(&mut lock, &lock_content)?;
    writeln!(out, "Installed {} managed skills", new_names.len())?;
    Ok(())
}
