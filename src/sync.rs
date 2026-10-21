use crate::{
    generation::{route_guides, set_skill_name, write_router},
    git::{acquire_dependency, cache_root, run_git},
    lock::{
        DependencyInput, GenerationInputs, GenerationState, ResolvedDependency, SyncRecord,
        render_sync_lock, sync_lock, write_sync_lock,
    },
    manifest::{Dependency, Manifest, Mapping, Router},
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

struct PreparedDependency {
    selected: Vec<SelectedSkill>,
    router: Option<(Router, Vec<String>)>,
    input: DependencyInput,
    resolved: ResolvedDependency,
}

struct PreparedSync {
    selected: Vec<SelectedSkill>,
    routers: Vec<(Router, Vec<String>)>,
    inputs: GenerationInputs,
    resolved: Vec<ResolvedDependency>,
    destinations: BTreeSet<String>,
}

fn read_previous_record(lock: &mut fs::File) -> Result<SyncRecord> {
    let mut content = Vec::new();
    lock.read_to_end(&mut content)?;
    // Full-file parsing rejects torn shorter writes with trailing old metadata.
    Ok(serde_json::from_slice::<SyncRecord>(&content)
        .ok()
        .filter(|record| record.version == 1)
        .unwrap_or_default())
}

fn acquire_dependencies(dependencies: &[Dependency], cache: &Path) -> Result<Vec<PathBuf>> {
    let next = AtomicUsize::new(0);
    let results: Vec<_> = dependencies.iter().map(|_| Mutex::new(None)).collect();
    thread::scope(|scope| -> Result<()> {
        let mut workers = Vec::new();
        for _ in 0..dependencies.len().min(4) {
            workers.push(
                scope.spawn(|| acquire_dependency_worker(&next, dependencies, cache, &results)),
            );
        }

        for worker in workers {
            worker
                .join()
                .map_err(|_| anyhow!("Git acquisition worker panicked"))??;
        }

        Ok(())
    })?;

    results
        .into_iter()
        .map(|result| {
            result
                .into_inner()
                .map_err(|_| anyhow!("acquisition result lock poisoned"))?
                .context("missing acquisition result")?
        })
        .collect()
}

fn acquire_dependency_worker(
    next: &AtomicUsize,
    dependencies: &[Dependency],
    cache: &Path,
    results: &[Mutex<Option<Result<PathBuf>>>],
) -> Result<()> {
    loop {
        let index = next.fetch_add(1, Ordering::Relaxed);
        let Some(dependency) = dependencies.get(index) else {
            break;
        };
        let result = acquire_dependency(dependency, cache);
        *results[index]
            .lock()
            .map_err(|_| anyhow!("acquisition result lock poisoned"))? = Some(result);
    }

    Ok(())
}

fn prepare_sync(manifest: &Manifest, clones: &[PathBuf]) -> Result<PreparedSync> {
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
    let mut prepared = PreparedSync {
        selected: Vec::new(),
        routers: Vec::new(),
        inputs,
        resolved: Vec::new(),
        destinations: BTreeSet::new(),
    };
    for (dependency, clone) in manifest.dependencies.iter().zip(clones) {
        let item = prepare_dependency(dependency, clone, &durable, &mut prepared.destinations)?;
        prepared.selected.extend(item.selected);
        if let Some(router) = item.router {
            prepared.routers.push(router);
        }

        prepared.inputs.dependencies.push(item.input);
        prepared.resolved.push(item.resolved);
    }

    Ok(prepared)
}

fn prepare_dependency(
    dependency: &Dependency,
    clone: &Path,
    durable: &BTreeSet<String>,
    destinations: &mut BTreeSet<String>,
) -> Result<PreparedDependency> {
    let mappings = dependency_mappings(dependency, clone)?;
    let router = dependency.router.as_ref().map(|router| {
        (
            router.clone(),
            mappings
                .iter()
                .map(|mapping| mapping.destination.clone())
                .collect(),
        )
    });
    if let Some((router, _)) = &router {
        ensure!(
            !durable.contains(&router.name) && destinations.insert(router.name.clone()),
            "duplicate destination or authored skill collision: {}",
            router.name
        );
    }

    let resolved_clone = fs::canonicalize(clone)?;
    let mut selected = Vec::new();
    let mut paths = Vec::new();
    let mut has_symlinks = false;
    for mapping in &mappings {
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
        has_symlinks |=
            inspect_skill_source(dependency, &source, &mapping.source, clone, &resolved_clone)?;
        selected.push(SelectedSkill {
            source,
            relative,
            repository: clone.to_owned(),
            routed: dependency.router.is_some(),
        });
        paths.push(mapping.source.clone());
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
    let revisions: Vec<_> = run_git(clone, &args.iter().map(String::as_str).collect::<Vec<_>>())?
        .lines()
        .map(str::to_owned)
        .collect();
    let reference = if dependency.reference.is_empty() {
        "HEAD".to_owned()
    } else {
        dependency.reference.clone()
    };
    Ok(PreparedDependency {
        selected,
        router,
        input: DependencyInput {
            config: dependency.clone(),
            trees: revisions[1..].to_vec(),
        },
        resolved: ResolvedDependency {
            id: dependency.id.clone(),
            url: dependency.url.clone(),
            reference,
            commit: revisions[0].clone(),
        },
    })
}

fn inspect_skill_source(
    dependency: &Dependency,
    source: &Path,
    source_label: &str,
    clone: &Path,
    resolved_clone: &Path,
) -> Result<bool> {
    ensure!(
        source.is_dir() && source.join("SKILL.md").is_file(),
        "{} source is not a skill: {}",
        dependency.id,
        source_label
    );
    let resolved_source = fs::canonicalize(source)?;
    ensure!(
        resolved_source.starts_with(resolved_clone),
        "source symlink escapes repository: {}",
        source.display()
    );
    let mut has_symlinks = false;
    for ancestor in source
        .ancestors()
        .take_while(|ancestor| ancestor.starts_with(clone))
    {
        has_symlinks |= fs::symlink_metadata(ancestor)?.file_type().is_symlink();
    }

    for entry in WalkDir::new(source) {
        let entry = entry?;
        if entry.file_type().is_symlink() {
            has_symlinks = true;
            let resolved = fs::canonicalize(entry.path())?;
            ensure!(
                resolved.starts_with(resolved_clone),
                "source symlink escapes repository: {}",
                entry.path().display()
            );
        }
    }

    Ok(has_symlinks)
}

fn dependency_mappings(dependency: &Dependency, clone: &Path) -> Result<Vec<Mapping>> {
    if dependency.selection.mode != "discovery" {
        return Ok(dependency.selection.include.clone());
    }

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
    entries
        .into_iter()
        .filter_map(|name| {
            let source = discovery_root.join(&name);
            (!dependency.selection.exclude.contains(&name)
                && source.is_dir()
                && source.join("SKILL.md").is_file())
            .then(|| {
                source.strip_prefix(clone).map(|path| Mapping {
                    source: path.to_string_lossy().into_owned(),
                    destination: name,
                })
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn stage_generated_skills(
    stage: &Path,
    selected: Vec<SelectedSkill>,
    routers: Vec<(Router, Vec<String>)>,
) -> Result<()> {
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

    Ok(())
}

fn report_changes(
    out: &mut dyn Write,
    current: &Path,
    stage: &Path,
    old_names: &[String],
    new_names: &[String],
) -> Result<()> {
    for name in new_names {
        if !old_names.contains(name) {
            writeln!(out, "ADD {name}")?;
        } else if !trees_equal(&current.join(name), &stage.join(name))? {
            writeln!(out, "UPDATE {name}")?;
        }
    }

    for name in old_names {
        if !new_names.contains(name) {
            writeln!(out, "REMOVE {name}")?;
        }
    }

    Ok(())
}

fn publish_generation(
    lock: &mut fs::File,
    temporary: tempfile::TempDir,
    stage: &Path,
    current: &Path,
    live_fingerprint: &str,
    record: &mut SyncRecord,
) -> Result<()> {
    record.state.output = tree_fingerprint(stage)?;
    let lock_content = render_sync_lock(record)?;
    if live_fingerprint != record.state.output {
        let backup = temporary.path().join("previous-managed");
        let moved_old = match fs::symlink_metadata(current) {
            Ok(_) => {
                fs::rename(current, &backup)?;
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        if let Err(error) = fs::rename(stage, current) {
            if moved_old && let Err(restore) = fs::rename(&backup, current) {
                let recovery = temporary.keep();
                bail!(
                    "install failed: {error}; restore failed: {restore}; backup retained in {}",
                    recovery.display()
                );
            }

            return Err(error.into());
        }
    }

    write_sync_lock(lock, &lock_content)
}

/// Resolve dependencies and publish the manifest's generated skills atomically.
///
/// # Errors
///
/// Returns an error when dependency acquisition, source validation, generation,
/// or publication fails.
pub fn sync(root: &Path, manifest: &Manifest, dry_run: bool, out: &mut dyn Write) -> Result<()> {
    let mut lock = sync_lock(root)?;
    let cache = cache_root()?;
    fs::create_dir_all(&cache)?;
    let _cache_lock = sync_lock(&cache)?;
    let previous = read_previous_record(&mut lock)?;
    let clones = acquire_dependencies(&manifest.dependencies, &cache)?;
    let temporary = Builder::new()
        .prefix(".skills-sync-")
        .tempdir_in(root.parent().context("skills root has no parent")?)?;
    let stage = temporary.path().join("_managed");
    fs::create_dir(&stage)?;
    let prepared = prepare_sync(manifest, &clones)?;
    let current = root.join("_managed");
    let live_fingerprint = tree_fingerprint(&current)?;
    let state = GenerationState {
        inputs: prepared.inputs,
        output: live_fingerprint.clone(),
    };
    let mut record = SyncRecord {
        version: 1,
        dependencies: prepared.resolved,
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
            prepared.destinations.len()
        )?;
        return Ok(());
    }

    stage_generated_skills(&stage, prepared.selected, prepared.routers)?;
    let old_names = directory_names(&current)?;
    let new_names = directory_names(&stage)?;
    report_changes(out, &current, &stage, &old_names, &new_names)?;

    if dry_run {
        writeln!(out, "Would install {} managed skills", new_names.len())?;
        return Ok(());
    }

    publish_generation(
        &mut lock,
        temporary,
        &stage,
        &current,
        &live_fingerprint,
        &mut record,
    )?;
    writeln!(out, "Installed {} managed skills", new_names.len())?;
    Ok(())
}
