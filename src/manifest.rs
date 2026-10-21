use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_yaml_ng::{Mapping as YamlMapping, Value};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub dependencies: Vec<Dependency>,
    pub durable_roots: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub id: String,
    pub url: String,
    #[serde(rename = "ref")]
    pub reference: String,
    pub sparse: Vec<String>,
    pub selection: Selection,
    pub claude: bool,
    pub router: Option<Router>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub mode: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub include: Vec<Mapping>,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub root: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub exclude: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mapping {
    pub source: String,
    pub destination: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Router {
    pub name: String,
    pub description: String,
}

/// Validate both manifests before acquiring repositories or publishing output.
///
/// # Errors
///
/// Returns an error when either manifest is invalid, unreadable, or conflicts with
/// authored skills or another dependency's destinations.
pub fn load_manifest(root: &Path) -> Result<Manifest> {
    let base = read_manifest_mapping(
        &root.join("deps.yaml"),
        &["version", "dependencies"],
        &["version", "dependencies"],
    )?;
    ensure!(version_one(&base["version"]), "deps.yaml version must be 1");
    let raw = base["dependencies"]
        .as_sequence()
        .context("deps.yaml dependencies must be a list")?;
    let mut manifest = Manifest::default();
    let mut base_ids = HashSet::new();
    for (index, raw) in raw.iter().enumerate() {
        let dependency = normalize_dependency(raw, &format!("deps.yaml.dependencies[{index}]"), true)?;
        ensure!(
            base_ids.insert(dependency.id.clone()),
            "deps.yaml contains duplicate dependency IDs"
        );
        manifest.dependencies.push(dependency);
    }

    apply_overlay(root, &mut manifest, &base_ids)?;
    collect_authored_roots(root, &mut manifest)?;
    validate_destinations(&manifest)?;
    Ok(manifest)
}

struct OverlaySections {
    additions: Vec<Value>,
    extensions: Vec<Value>,
    removals: Vec<Value>,
}

fn apply_overlay(root: &Path, manifest: &mut Manifest, base_ids: &HashSet<String>) -> Result<()> {
    let overlay_path = root.join("deps.local.yaml");
    if !overlay_path
        .try_exists()
        .context("cannot read deps.local.yaml")?
    {
        return Ok(());
    }

    let overlay = read_manifest_mapping(&overlay_path, &["version", "add", "extend", "remove"], &["version"])?;
    ensure!(version_one(&overlay["version"]), "deps.local.yaml version must be 1");
    let sections = overlay_sections(&overlay)?;
    add_dependencies(manifest, &sections.additions)?;

    let by_id = manifest
        .dependencies
        .iter()
        .enumerate()
        .map(|(index, dependency)| (dependency.id.clone(), index))
        .collect();
    extend_dependencies(manifest, &by_id, &sections.extensions)?;
    remove_dependencies(manifest, base_ids, &sections.removals)
}

fn overlay_sections(overlay: &YamlMapping) -> Result<OverlaySections> {
    let section = |name: &str, missing_message: &str| -> Result<Vec<Value>> {
        match overlay.get(name) {
            Some(value) => Ok(value
                .as_sequence()
                .with_context(|| missing_message.to_owned())?
                .clone()),
            None => Ok(Vec::new()),
        }
    };

    Ok(OverlaySections {
        additions: section("add", "deps.local.yaml add and extend must be lists")?,
        extensions: section("extend", "deps.local.yaml add and extend must be lists")?,
        removals: section("remove", "deps.local.yaml remove must be a list")?,
    })
}

fn add_dependencies(manifest: &mut Manifest, additions: &[Value]) -> Result<()> {
    let mut ids: HashSet<_> = manifest
        .dependencies
        .iter()
        .map(|dependency| dependency.id.clone())
        .collect();
    for (index, raw) in additions.iter().enumerate() {
        let addition = normalize_dependency(raw, &format!("deps.local.yaml.add[{index}]"), false)?;
        ensure!(
            ids.insert(addition.id.clone()),
            "deps.local.yaml add redefines dependency {}",
            addition.id
        );
        manifest.dependencies.push(addition);
    }

    Ok(())
}

fn extend_dependencies(manifest: &mut Manifest, by_id: &HashMap<String, usize>, extensions: &[Value]) -> Result<()> {
    for (index, raw) in extensions.iter().enumerate() {
        let context = format!("deps.local.yaml.extend[{index}]");
        let extension = manifest_mapping(raw, &context, &["id", "include"], &["id", "include"])?;
        let id = normalized_name(&extension["id"], &format!("{context}.id"))?;
        let dependency_index = by_id
            .get(&id)
            .with_context(|| format!("deps.local.yaml extend names unknown dependency {id}"))?;
        let dependency = &mut manifest.dependencies[*dependency_index];
        ensure!(
            dependency.selection.mode == "explicit",
            "deps.local.yaml cannot extend discovery dependency {id}"
        );
        let extra = normalize_include(&extension["include"], &format!("{context}.include"))?;
        for mapping in &extra {
            ensure!(
                dependency.sparse.is_empty() || beneath_sparse(&mapping.source, &dependency.sparse),
                "deps.local.yaml source {} is outside {id} sparse paths",
                mapping.source
            );
            ensure!(
                !dependency.selection.include.contains(mapping),
                "deps.local.yaml repeats a mapping for {id}"
            );
            ensure!(
                !dependency
                    .selection
                    .include
                    .iter()
                    .any(|existing| existing.destination == mapping.destination),
                "duplicate destination {} in {id}",
                mapping.destination
            );
        }

        dependency.selection.include.extend(extra);
    }

    Ok(())
}

fn remove_dependencies(manifest: &mut Manifest, base_ids: &HashSet<String>, removals: &[Value]) -> Result<()> {
    let mut removed = HashSet::new();
    for (index, raw) in removals.iter().enumerate() {
        let id = normalized_name(raw, &format!("deps.local.yaml.remove[{index}]"))?;
        ensure!(
            base_ids.contains(&id),
            "deps.local.yaml remove names unknown base dependency {id}"
        );
        removed.insert(id);
    }

    manifest
        .dependencies
        .retain(|dependency| !removed.contains(&dependency.id));
    Ok(())
}

fn collect_authored_roots(root: &Path, manifest: &mut Manifest) -> Result<()> {
    for entry in fs::read_dir(root).context("cannot read authored skills")? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != "_managed" && entry.path().is_dir() && entry.path().join("SKILL.md").is_file() {
            manifest.durable_roots.push(name);
        }
    }

    manifest.durable_roots.sort();
    Ok(())
}

fn validate_destinations(manifest: &Manifest) -> Result<()> {
    let mut owners = HashMap::new();
    for dependency in &manifest.dependencies {
        for destination in top_level_names(dependency) {
            ensure!(
                !manifest.durable_roots.contains(&destination),
                "managed destination {destination} collides with an authored skill"
            );
            if let Some(owner) = owners.insert(destination.clone(), &dependency.id) {
                bail!("duplicate destination {destination} in {owner} and {}", dependency.id);
            }
        }
    }

    Ok(())
}

fn read_manifest_mapping(path: &Path, allowed: &[&str], required: &[&str]) -> Result<YamlMapping> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let data = fs::read_to_string(path).with_context(|| format!("cannot read {name}"))?;
    let data = legacy_yaml_booleans(&data).with_context(|| format!("cannot read {name}"))?;
    let mut documents = serde_yaml_ng::Deserializer::from_str(&data);
    let raw = match documents.next() {
        Some(document) => Value::deserialize(document).with_context(|| format!("cannot read {name}"))?,
        None => Value::Null,
    };
    ensure!(
        documents.next().is_none(),
        "cannot read {name}: expected a single document"
    );
    Ok(manifest_mapping(&raw, &name, allowed, required)?.clone())
}

fn manifest_mapping<'a>(raw: &'a Value, context: &str, allowed: &[&str], required: &[&str]) -> Result<&'a YamlMapping> {
    let mapping = raw
        .as_mapping()
        .with_context(|| format!("{context} must be a mapping"))?;
    let mut unknown = BTreeSet::new();
    for key in mapping.keys() {
        let key = key
            .as_str()
            .with_context(|| format!("{context} must use string keys"))?;
        if !allowed.contains(&key) {
            unknown.insert(key);
        }
    }

    ensure!(
        unknown.is_empty(),
        "{context} has unknown keys: {}",
        unknown.into_iter().collect::<Vec<_>>().join(", ")
    );
    let missing: BTreeSet<_> = required
        .iter()
        .filter(|key| !mapping.contains_key(**key))
        .copied()
        .collect();
    ensure!(
        missing.is_empty(),
        "{context} is missing keys: {}",
        missing.into_iter().collect::<Vec<_>>().join(", ")
    );
    Ok(mapping)
}

fn version_one(value: &Value) -> bool {
    value.as_bool() == Some(true) || value.as_f64() == Some(1.0)
}

/// Validate a single safe skill or destination directory name.
///
/// # Errors
///
/// Returns an error when the name is empty, reserved, hidden, or contains a path
/// separator or NUL byte.
pub fn safe_name(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty() && value != "_managed" && !value.starts_with('.') && !value.contains(['/', '\\', '\0']),
        "must be a safe directory name"
    );
    Ok(())
}

fn normalized_name(raw: &Value, context: &str) -> Result<String> {
    let value = raw
        .as_str()
        .with_context(|| format!("{context} must be a safe directory name"))?;
    safe_name(value).with_context(|| format!("{context} must be a safe directory name"))?;
    Ok(value.to_owned())
}

fn safe_repo_path(raw: &Value, context: &str, allow_dot: bool) -> Result<String> {
    let value = raw
        .as_str()
        .filter(|value| !value.is_empty() && !value.contains(['\0', '\\']))
        .with_context(|| format!("{context} must be a non-empty POSIX path"))?;
    ensure!(
        !value.starts_with('/') && !value.split('/').any(|part| part.is_empty() || part == ".."),
        "{context} must stay inside the repository"
    );
    if value == "." {
        ensure!(allow_dot, "{context} cannot be .");
    } else {
        ensure!(
            !value.split('/').any(|part| part == "."),
            "{context} must be normalized"
        );
    }

    Ok(value.to_owned())
}

fn unique_manifest_strings(raw: &Value, context: &str, paths: bool) -> Result<Vec<String>> {
    let values = raw
        .as_sequence()
        .filter(|values| values.iter().all(Value::is_string))
        .with_context(|| format!("{context} must be a list of strings"))?;
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let normalized = if paths {
            safe_repo_path(value, &format!("{context} entry"), false)?
        } else {
            normalized_name(value, &format!("{context} entry"))?
        };
        ensure!(seen.insert(normalized.clone()), "{context} contains duplicates");
        result.push(normalized);
    }

    Ok(result)
}

fn beneath_sparse(path: &str, prefixes: &[String]) -> bool {
    prefixes
        .iter()
        .any(|prefix| prefix == "." || path == prefix || path.starts_with(&format!("{prefix}/")))
}

fn normalize_include(raw: &Value, context: &str) -> Result<Vec<Mapping>> {
    let values = raw
        .as_sequence()
        .filter(|values| !values.is_empty())
        .with_context(|| format!("{context} must be a non-empty list"))?;
    let mut result = Vec::new();
    let mut pairs = HashSet::new();
    let mut destinations = HashSet::new();
    for (index, raw) in values.iter().enumerate() {
        let context = format!("{context}[{index}]");
        let item = manifest_mapping(raw, &context, &["source", "destination"], &["source", "destination"])?;
        let mapping = Mapping {
            source: safe_repo_path(&item["source"], &format!("{context}.source"), true)?,
            destination: normalized_name(&item["destination"], &format!("{context}.destination"))?,
        };
        ensure!(pairs.insert(mapping.clone()), "{context} contains duplicate mappings");
        ensure!(
            destinations.insert(mapping.destination.clone()),
            "{context} contains duplicate destinations"
        );
        result.push(mapping);
    }

    Ok(result)
}

fn normalize_dependency(raw: &Value, context: &str, base: bool) -> Result<Dependency> {
    let item = manifest_mapping(
        raw,
        context,
        &["id", "url", "ref", "sparse", "selection", "claude", "router"],
        &["id", "url", "selection"],
    )?;
    let mut dependency = Dependency {
        id: normalized_name(&item["id"], &format!("{context}.id"))?,
        url: item["url"]
            .as_str()
            .with_context(|| format!("{context}.url must be a string"))?
            .to_owned(),
        claude: true,
        ..Dependency::default()
    };
    if base {
        ensure!(
            dependency.url.starts_with("https://github.com/") && dependency.url.ends_with(".git"),
            "{context}.url must be an HTTPS GitHub URL"
        );
    } else {
        ensure!(
            ["https://", "ssh://", "git@"]
                .iter()
                .any(|prefix| dependency.url.starts_with(prefix)),
            "{context}.url must use HTTPS or SSH"
        );
    }

    if let Some(reference) = item.get("ref").filter(|value| !value.is_null()) {
        dependency.reference = reference
            .as_str()
            .filter(|value| !value.is_empty())
            .with_context(|| format!("{context}.ref must be a non-empty string"))?
            .to_owned();
    }

    if let Some(sparse) = item.get("sparse").filter(|value| !value.is_null()) {
        dependency.sparse = unique_manifest_strings(sparse, &format!("{context}.sparse"), true)?;
        ensure!(!dependency.sparse.is_empty(), "{context}.sparse must not be empty");
    }

    if let Some(claude) = item.get("claude") {
        dependency.claude = claude
            .as_bool()
            .with_context(|| format!("{context}.claude must be a boolean"))?;
    }

    if let Some(raw_router) = item.get("router").filter(|value| !value.is_null()) {
        let router = manifest_mapping(
            raw_router,
            &format!("{context}.router"),
            &["name", "description"],
            &["name", "description"],
        )?;
        let description = router["description"]
            .as_str()
            .filter(|description| !description.trim().is_empty())
            .with_context(|| format!("{context}.router.description must be a string"))?;
        dependency.router = Some(Router {
            name: normalized_name(&router["name"], &format!("{context}.router.name"))?,
            description: description.split_whitespace().collect::<Vec<_>>().join(" "),
        });
    }

    let selection = normalize_selection(&item["selection"], context, &dependency.sparse)?;
    ensure!(
        dependency.claude || selection.mode == "explicit" || dependency.router.is_some(),
        "{context}.claude: false requires explicit selection or a router"
    );
    dependency.selection = selection;
    Ok(dependency)
}

fn normalize_selection(raw: &Value, context: &str, sparse: &[String]) -> Result<Selection> {
    let mapping = raw
        .as_mapping()
        .with_context(|| format!("{context}.selection must be a mapping"))?;
    let mode = mapping
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut selection = Selection {
        mode: mode.to_owned(),
        ..Selection::default()
    };
    match mode {
        "explicit" => {
            manifest_mapping(
                raw,
                &format!("{context}.selection"),
                &["mode", "include"],
                &["mode", "include"],
            )?;
            selection.include = normalize_include(&mapping["include"], &format!("{context}.selection.include"))?;
            for item in &selection.include {
                ensure!(
                    sparse.is_empty() || beneath_sparse(&item.source, sparse),
                    "{context} source {} is outside sparse paths",
                    item.source
                );
            }
        }
        "discovery" => {
            manifest_mapping(
                raw,
                &format!("{context}.selection"),
                &["mode", "root", "exclude"],
                &["mode", "root"],
            )?;
            selection.root = safe_repo_path(&mapping["root"], &format!("{context}.selection.root"), true)?;
            if let Some(exclude) = mapping.get("exclude") {
                selection.exclude = unique_manifest_strings(exclude, &format!("{context}.selection.exclude"), false)?;
            }

            ensure!(
                sparse.is_empty() || beneath_sparse(&selection.root, sparse),
                "{context} discovery root {} is outside sparse paths",
                selection.root
            );
        }
        _ => bail!("{context}.selection.mode must be explicit or discovery"),
    }

    Ok(selection)
}

pub fn top_level_names(dependency: &Dependency) -> Vec<String> {
    if let Some(router) = &dependency.router {
        return vec![router.name.clone()];
    }

    if dependency.selection.mode == "explicit" {
        return dependency
            .selection
            .include
            .iter()
            .map(|mapping| mapping.destination.clone())
            .collect();
    }

    Vec::new()
}

/// Preserve YAML 1.1 booleans without coercing quoted or explicitly tagged strings.
fn legacy_yaml_booleans(source: &str) -> Result<String> {
    use yaml_rust2::parser::{Event, Parser};
    use yaml_rust2::scanner::TScalarStyle;

    let mut parser = Parser::new_from_str(source);
    let byte_offsets: Vec<_> = source
        .char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(source.len()))
        .collect();
    let mut replacements = Vec::new();
    loop {
        let (event, marker) = parser.next_token()?;
        match event {
            Event::Scalar(value, TScalarStyle::Plain, _, None) => {
                let replacement = match value.as_str() {
                    "yes" | "Yes" | "YES" | "on" | "On" | "ON" => "true",
                    "no" | "No" | "NO" | "off" | "Off" | "OFF" => "false",
                    _ => continue,
                };
                let offset = byte_offsets[marker.index()];
                ensure!(
                    source[offset..].starts_with(&value),
                    "invalid legacy YAML scalar position"
                );
                replacements.push((offset, value.len(), replacement));
            }
            Event::StreamEnd => break,
            _ => {}
        }
    }

    let mut normalized = source.to_owned();
    for (offset, length, replacement) in replacements.into_iter().rev() {
        normalized.replace_range(offset..offset + length, replacement);
    }

    Ok(normalized)
}
