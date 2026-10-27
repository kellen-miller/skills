use super::*;

pub(super) fn text(value: &Value) -> Result<&str> {
    value.as_str().context("expected a string")
}

pub(super) fn strings(value: &Value) -> Result<Vec<String>> {
    value
        .as_array()
        .context("expected an array")?
        .iter()
        .map(|v| Ok(text(v)?.to_owned()))
        .collect()
}

pub(super) fn command(value: &Value) -> Result<Vec<String>> {
    let args = strings(value).context("commands must be nonempty argv arrays")?;
    ensure!(
        !args.is_empty() && args.iter().all(|s| !s.is_empty() && !s.contains('\0')),
        "commands must be nonempty argv arrays"
    );
    Ok(args)
}

pub(super) fn checks(value: &Value) -> Result<Vec<Vec<String>>> {
    let values = value.as_array().context("requires nonempty checks")?;
    ensure!(!values.is_empty(), "requires nonempty checks");
    values.iter().map(command).collect()
}

pub(super) fn ignored_file(repo: &Path, path: &Path) -> Result<PathBuf> {
    let path = path
        .canonicalize()
        .context("graph and briefs must be files under .agent/work/")?;
    let relative = path
        .strip_prefix(repo)
        .context("graph and briefs must be files under .agent/work/")?;
    ensure!(
        relative.starts_with(".agent/work") && path.is_file(),
        "graph and briefs must be files under .agent/work/"
    );
    git(
        repo,
        &[
            "check-ignore",
            "-q",
            "--",
            path.to_str().context("non-UTF8 input path")?,
        ],
    )
    .context("graph and briefs must be Git ignored")?;
    Ok(path)
}

pub(super) fn owned_path(value: &Value) -> Result<String> {
    let value = text(value).context("owned paths must be nonempty repository-relative paths")?;
    let path = value.trim_end_matches('/');
    ensure!(
        !path.is_empty()
            && !value.chars().any(|c| "\\*?[]\n\r\0".contains(c))
            && !path
                .split('/')
                .any(|p| matches!(p, "" | "." | ".." | ".git" | ".agent")),
        "unsafe owned path: {value}"
    );
    Ok(path.to_owned())
}

// Preserve the original dispatcher's sorted, ASCII JSON fingerprint, including
// separator spaces, so recorded runs survive the runtime migration.
pub(super) fn fingerprint_json(value: &Value) -> Result<String> {
    match value {
        Value::Number(number) if number.is_f64() => {
            let value = number.as_f64().context("invalid graph number")?;
            if value == 0.0 || (1e-4..1e16).contains(&value.abs()) {
                let mut encoded = value.to_string();
                if !encoded.contains('.') {
                    encoded.push_str(".0");
                }

                Ok(encoded)
            } else {
                let encoded = format!("{value:e}");
                let (mantissa, exponent) = encoded.split_once('e').context("invalid exponent")?;
                let exponent: i32 = exponent.parse()?;
                Ok(format!("{mantissa}e{exponent:+03}"))
            }
        }
        Value::String(value) => {
            let encoded = serde_json::to_string(value)?;
            let mut ascii = String::new();
            for character in encoded.chars() {
                if character.is_ascii() && character != '\u{7f}' {
                    ascii.push(character);
                } else {
                    for unit in character.encode_utf16(&mut [0; 2]) {
                        ascii.push_str(&format!("\\u{unit:04x}"));
                    }
                }
            }

            Ok(ascii)
        }
        Value::Array(values) => Ok(format!(
            "[{}]",
            values
                .iter()
                .map(fingerprint_json)
                .collect::<Result<Vec<_>>>()?
                .join(", ")
        )),
        Value::Object(values) => Ok(format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| Ok(format!(
                    "{}: {}",
                    fingerprint_json(&json!(key))?,
                    fingerprint_json(value)?
                )))
                .collect::<Result<Vec<_>>>()?
                .join(", ")
        )),
        _ => Ok(serde_json::to_string(value)?),
    }
}

pub(super) fn load_graph(repo: &Path, path: &Path) -> Result<Graph> {
    let path = ignored_file(repo, path)?;
    let raw: Value = serde_json::from_slice(&fs::read(&path)?)?;
    ensure!(
        raw.is_object() && raw["version"] == 1 && raw["approved"] == true,
        "graph requires version=1 and approved=true"
    );
    let base_ref = text(&raw["base_ref"])?;
    let branch = text(&raw["integration_branch"])?;
    ensure!(
        base_ref.starts_with("refs/") && branch.starts_with("refs/heads/"),
        "base_ref and integration_branch require explicit Git refs"
    );
    let base = commit(repo, base_ref)?;
    git(repo, &["check-ref-format", branch])?;
    ensure!(
        git_text(repo, &["symbolic-ref", "HEAD"])? == branch,
        "checkout is not on the graph's integration branch"
    );
    ensure!(
        ancestor(repo, &base, &commit(repo, "HEAD")?)?,
        "integration HEAD does not descend from base_ref"
    );
    let worker = command(&raw["worker_command"])?;
    let integration_checks =
        checks(&raw["integration_checks"]).context("graph requires integration_checks")?;
    let entries = raw["tickets"]
        .as_array()
        .context("graph requires nonempty tickets")?;
    ensure!(!entries.is_empty(), "graph requires nonempty tickets");
    let slug = Regex::new(r"^[a-z0-9][a-z0-9-]{0,31}$")?;
    let subject = Regex::new(
        r"^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\([a-z0-9-]+\))?: [^\r\n]+$",
    )?;
    let mut tickets = BTreeMap::new();
    let mut inputs = Vec::new();
    for entry in entries {
        let (id, ticket, input) = parse_ticket(repo, &path, entry, &worker, &slug, &subject)?;
        ensure!(
            !tickets.contains_key(&id),
            "ticket IDs must be unique lowercase slugs, at most 32 chars"
        );
        inputs.push(input);
        tickets.insert(id, ticket);
    }

    validate_ticket_graph(&tickets)?;

    let digest = format!(
        "{:x}",
        Sha256::digest(fingerprint_json(&json!([raw, inputs]))?.as_bytes())
    );
    Ok(Graph {
        path,
        branch: branch.to_owned(),
        checks: integration_checks,
        tickets,
        digest,
    })
}

pub(super) fn parse_ticket(
    repo: &Path,
    graph_path: &Path,
    entry: &Value,
    default_worker: &[String],
    slug: &Regex,
    subject: &Regex,
) -> Result<(String, Ticket, Value)> {
    ensure!(entry.is_object(), "tickets must be objects");
    let id = text(&entry["id"])?;
    ensure!(
        slug.is_match(id),
        "ticket IDs must be unique lowercase slugs, at most 32 chars"
    );
    let deliverable =
        text(&entry["deliverable"]).with_context(|| format!("{id} requires a deliverable"))?;
    ensure!(
        !deliverable.trim().is_empty(),
        "{id} requires a deliverable"
    );
    let dependencies = strings(&entry["depends_on"])
        .with_context(|| format!("{id} requires depends_on as an ID list"))?;
    ensure!(
        dependencies.iter().collect::<BTreeSet<_>>().len() == dependencies.len(),
        "{id} has duplicate dependencies"
    );
    let paths = entry["owned_paths"]
        .as_array()
        .with_context(|| format!("{id} requires owned_paths"))?;
    ensure!(!paths.is_empty(), "{id} requires owned_paths");
    let paths = paths.iter().map(owned_path).collect::<Result<Vec<_>>>()?;
    let message = text(&entry["commit_message"])?;
    ensure!(
        message.chars().count() <= 50 && subject.is_match(message),
        "{id} requires a Conventional Commit subject of at most 50 chars"
    );
    let worker = if entry.get("worker_command").is_some() {
        command(&entry["worker_command"])?
    } else {
        default_worker.to_owned()
    };
    let checks = checks(&entry["checks"]).with_context(|| format!("{id} requires checks"))?;
    let brief_path = ignored_file(
        repo,
        &graph_path
            .parent()
            .context("graph has no parent")?
            .join(text(&entry["brief"])?),
    )?;
    let brief = fs::read_to_string(brief_path)?
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    ensure!(!brief.trim().is_empty(), "{id} brief is empty");
    Ok((
        id.to_owned(),
        Ticket {
            deliverable: deliverable.to_owned(),
            dependencies,
            paths,
            message: message.to_owned(),
            worker,
            checks,
            brief: brief.clone(),
        },
        json!([id, brief]),
    ))
}

pub(super) fn validate_ticket_graph(tickets: &BTreeMap<String, Ticket>) -> Result<()> {
    ensure!(
        tickets
            .values()
            .flat_map(|ticket| &ticket.dependencies)
            .all(|dependency| tickets.contains_key(dependency)),
        "graph references an unknown dependency"
    );
    let mut remaining: BTreeSet<_> = tickets.keys().cloned().collect();
    while !remaining.is_empty() {
        let ready: Vec<_> = remaining
            .iter()
            .filter(|id| {
                tickets[*id]
                    .dependencies
                    .iter()
                    .all(|dependency| !remaining.contains(dependency))
            })
            .cloned()
            .collect();
        ensure!(!ready.is_empty(), "graph has a dependency cycle");
        for id in ready {
            remaining.remove(&id);
        }
    }

    Ok(())
}
