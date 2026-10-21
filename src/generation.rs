use crate::manifest::Router;
use anyhow::{Context, Result, anyhow};
use minijinja::{Environment, UndefinedBehavior};
use regex::bytes::Regex;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;
use walkdir::WalkDir;

type CompiledRegex = LazyLock<std::result::Result<Regex, regex::Error>>;

static FRONTMATTER: CompiledRegex = LazyLock::new(|| Regex::new(r"(?sm-u)\A---\n(.*?)^---[ \t]*$"));
static NAME_LINE: CompiledRegex = LazyLock::new(|| Regex::new(r"(?m-u)^name:.*$"));
static GUIDE_REFERENCE: CompiledRegex = LazyLock::new(|| Regex::new(r#"(?-u)[^\s`()<>"']*SKILL\.md"#));
static URL_SCHEME: CompiledRegex = LazyLock::new(|| Regex::new(r"\A[A-Za-z][A-Za-z0-9+.-]*:"));

fn compiled_regex(regex: &'static CompiledRegex, name: &str) -> Result<&'static Regex> {
    regex
        .as_ref()
        .map_err(|error| anyhow!("invalid {name} regular expression: {error}"))
}

/// Render into memory; callers can only publish a complete document.
///
/// # Errors
///
/// Returns an error when the named template is missing, has an undefined value,
/// or cannot render the supplied data.
pub fn render_template(name: &str, data: Value) -> Result<Vec<u8>> {
    let mut environment = Environment::new();
    environment.set_undefined_behavior(UndefinedBehavior::Strict);
    // Every template owns its JSON escaping through the serialization filter.
    environment.set_auto_escape_callback(|_| minijinja::AutoEscape::None);
    environment.set_syntax(
        minijinja::syntax::SyntaxConfig::builder()
            .keep_trailing_newline(true)
            .build()?,
    );
    environment.add_filter(
        "json",
        |value: minijinja::Value| -> std::result::Result<String, minijinja::Error> {
            if value.is_undefined() {
                return Err(minijinja::Error::new(
                    minijinja::ErrorKind::UndefinedError,
                    "cannot encode an undefined template value",
                ));
            }

            let encoded = serde_json::to_string(&value)
                .map_err(|error| minijinja::Error::new(minijinja::ErrorKind::InvalidOperation, error.to_string()))?;
            let mut ascii = String::new();
            for character in encoded.chars() {
                if character < '\u{7f}' {
                    ascii.push(character);
                } else {
                    for unit in character.encode_utf16(&mut [0; 2]) {
                        ascii.push_str(&format!("\\u{unit:04x}"));
                    }
                }
            }

            Ok(ascii)
        },
    );
    environment.add_template("router.md.j2", include_str!("../templates/router.md.j2"))?;
    environment.add_template("index.md.j2", include_str!("../templates/index.md.j2"))?;
    environment.add_template("lock.json.j2", include_str!("../templates/lock.json.j2"))?;
    Ok(environment
        .get_template(name)?
        .render(minijinja::value::Serde(data))?
        .into_bytes())
}

/// Set the generated skill name while preserving the rest of its Markdown bytes.
///
/// # Errors
///
/// Returns an error when the skill cannot be read, its frontmatter patterns are
/// invalid, or the updated file cannot be written.
pub fn set_skill_name(dir: &Path, name: &str) -> Result<()> {
    let path = dir.join("SKILL.md");
    let original = fs::read(&path)?;
    let line = format!("name: {name}");
    let frontmatter = compiled_regex(&FRONTMATTER, "frontmatter")?;
    let name_line = compiled_regex(&NAME_LINE, "name line")?;
    let text = if let Some(captures) = frontmatter.captures(&original) {
        let header = captures.get(1).context("frontmatter capture is missing")?;
        let header_text = if name_line.is_match(header.as_bytes()) {
            name_line
                .replace(header.as_bytes(), regex::bytes::NoExpand(line.as_bytes()))
                .into_owned()
        } else {
            [line.as_bytes(), b"\n", header.as_bytes()].concat()
        };
        [b"---\n", header_text.as_slice(), &original[header.end()..]].concat()
    } else {
        [b"---\n", line.as_bytes(), b"\n---\n", &original].concat()
    };
    if text != original {
        fs::write(path, text)?;
    }

    Ok(())
}

#[derive(Serialize)]
struct RouterEntry {
    name: String,
    summary: String,
}

#[derive(Serialize)]
struct RouterIndex<'a> {
    filename: String,
    names: Vec<String>,
    entries: &'a [RouterEntry],
}

/// Generate a router skill and optional bounded index pages.
///
/// # Errors
///
/// Returns an error when a member guide is missing, frontmatter is invalid,
/// templates cannot render, or generated pages cannot be written.
pub fn write_router(dir: &Path, router: &Router, members: &[String]) -> Result<()> {
    let frontmatter = compiled_regex(&FRONTMATTER, "frontmatter")?;
    let mut names = members.to_owned();
    names.sort();
    let mut entries = Vec::new();
    let mut size = 0;
    for name in names {
        let content = fs::read(dir.join("skills").join(&name).join("GUIDE.md"))?;
        let mut summary = String::new();
        if let Some(captures) = frontmatter.captures(&content)
            && let Ok(metadata) = serde_yaml_ng::from_slice::<serde_yaml_ng::Value>(&captures[1])
            && let Some(description) = metadata.get("description")
        {
            match description {
                serde_yaml_ng::Value::String(text) => summary = text.clone(),
                serde_yaml_ng::Value::Bool(true) => summary = "true".to_owned(),
                serde_yaml_ng::Value::Number(number) if number.as_f64() != Some(0.0) => summary = number.to_string(),
                _ => {}
            }

            summary = summary.split_whitespace().collect::<Vec<_>>().join(" ");
        }

        size += format!("- `{name}` (`skills/{name}/GUIDE.md`)")
            .chars()
            .count()
            + summary.chars().count();
        entries.push(RouterEntry { name, summary });
    }

    let mut indexes = Vec::new();
    if entries.len() > 12 || size > 8000 {
        for group in entries.chunks(12) {
            let first = group.first().context("router index group is empty")?;
            let last = group.last().context("router index group is empty")?;
            indexes.push(RouterIndex {
                filename: format!("{}--{}.md", first.name, last.name),
                names: group
                    .iter()
                    .map(|entry| format!("`{}`", entry.name))
                    .collect(),
                entries: group,
            });
        }
    }

    let content = render_template(
        "router.md.j2",
        json!({"router": router, "entries": entries, "indexes": indexes}),
    )?;
    let mut index_contents = Vec::new();
    for index in &indexes {
        index_contents.push(render_template(
            "index.md.j2",
            json!({
                "router": router,
                "first": index.entries[0].name,
                "last": index.entries[index.entries.len() - 1].name,
                "entries": index.entries,
            }),
        )?);
    }

    if !indexes.is_empty() {
        fs::create_dir_all(dir.join("indexes"))?;
        for (index, content) in indexes.iter().zip(index_contents) {
            fs::write(dir.join("indexes").join(&index.filename), content)?;
        }
    }

    fs::write(dir.join("SKILL.md"), content)?;
    Ok(())
}

/// Rename skill documents and rewrite relative references to their new paths.
///
/// # Errors
///
/// Returns an error when a guide collision exists, a file cannot be read or
/// rewritten, or a skill guide cannot be renamed.
pub fn route_guides(dir: &Path) -> Result<()> {
    let guide_reference = compiled_regex(&GUIDE_REFERENCE, "guide reference")?;
    let url_scheme = compiled_regex(&URL_SCHEME, "URL scheme")?;
    let mut skills = Vec::new();
    let mut documents = Vec::new();
    let mut renamed = HashSet::new();
    for entry in WalkDir::new(dir).sort_by_file_name() {
        let entry = entry?;
        if entry.file_type().is_dir() {
            continue;
        }

        if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "md")
        {
            documents.push(entry.path().to_owned());
        }

        if entry.file_name() != "SKILL.md" {
            continue;
        }

        let guide = entry.path().with_file_name("GUIDE.md");
        match fs::symlink_metadata(&guide) {
            Ok(_) => anyhow::bail!("routed guide already exists: {}", guide.display()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }

        renamed.insert(entry.path().canonicalize()?);
        skills.push(entry.path().to_owned());
    }

    for document in documents {
        let parent = document
            .parent()
            .context("Markdown document has no parent")?;
        let content = fs::read(&document)?;
        let rewritten = guide_reference.replace_all(&content, |captures: &regex::bytes::Captures<'_>| {
            let target = &captures[0];
            let Ok(target_text) = std::str::from_utf8(target) else {
                return target.to_vec();
            };
            if Path::new(target_text).is_absolute() || target_text.contains("://") || url_scheme.is_match(target) {
                return target.to_vec();
            }

            let resolved = parent.join(target_text).canonicalize();
            if resolved.as_ref().is_ok_and(|path| renamed.contains(path)) {
                let mut replacement = target[..target.len() - "SKILL.md".len()].to_vec();
                replacement.extend_from_slice(b"GUIDE.md");
                return replacement;
            }

            target.to_vec()
        });
        if rewritten != content {
            fs::write(&document, rewritten)?;
        }
    }

    for path in skills {
        fs::rename(&path, path.with_file_name("GUIDE.md"))
            .with_context(|| format!("cannot route {}", path.display()))?;
    }

    Ok(())
}
