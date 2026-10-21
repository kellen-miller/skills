//! Execute approved immutable ticket graphs with durable local Git evidence.
use anyhow::{Context, Result, anyhow, bail, ensure};
use clap::{Parser, Subcommand};
use fs2::FileExt;
use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex, MutexGuard, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

static INTERRUPTED: OnceLock<Arc<AtomicBool>> = OnceLock::new();
fn interruption() -> &'static Arc<AtomicBool> {
    INTERRUPTED.get_or_init(|| Arc::new(AtomicBool::new(false)))
}

#[derive(Parser)]
#[command(about = "Run an approved ticket graph through isolated CLI workers")]
struct Arguments {
    #[arg(long)]
    repo: PathBuf,
    #[arg(long)]
    graph: PathBuf,
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    Status,
    Run {
        #[arg(long)]
        jobs: usize,
        #[arg(long, default_value_t = 1800.0)]
        worker_timeout: f64,
        #[arg(long, default_value_t = 300.0)]
        check_timeout: f64,
        #[arg(long)]
        retry: Vec<String>,
        #[arg(long)]
        accept_head: Option<String>,
    },
}

struct Ticket {
    deliverable: String,
    dependencies: Vec<String>,
    paths: Vec<String>,
    message: String,
    worker: Vec<String>,
    checks: Vec<Vec<String>>,
    brief: String,
}

struct Graph {
    path: PathBuf,
    branch: String,
    checks: Vec<Vec<String>>,
    tickets: BTreeMap<String, Ticket>,
    digest: String,
}

#[derive(Serialize, Deserialize)]
struct State {
    version: u64,
    graph_digest: String,
    integration_head: String,
    tasks: BTreeMap<String, Value>,
    adopted_heads: Vec<Value>,
}

fn git(repo: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()?;
    ensure!(
        output.status.success(),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(output.stdout)
}

fn git_text(repo: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8(git(repo, args)?)?.trim().to_owned())
}

fn commit(repo: &Path, reference: &str) -> Result<String> {
    git_text(repo, &["rev-parse", "--verify", &format!("{reference}^{{commit}}")])
}

fn ancestor(repo: &Path, earlier: &str, later: &str) -> Result<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["merge-base", "--is-ancestor", earlier, later])
        .output()?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => bail!("cannot verify commit ancestry"),
    }
}

fn text(value: &Value) -> Result<&str> {
    value.as_str().context("expected a string")
}

fn strings(value: &Value) -> Result<Vec<String>> {
    value
        .as_array()
        .context("expected an array")?
        .iter()
        .map(|v| Ok(text(v)?.to_owned()))
        .collect()
}

fn command(value: &Value) -> Result<Vec<String>> {
    let args = strings(value).context("commands must be nonempty argv arrays")?;
    ensure!(
        !args.is_empty() && args.iter().all(|s| !s.is_empty() && !s.contains('\0')),
        "commands must be nonempty argv arrays"
    );
    Ok(args)
}

fn checks(value: &Value) -> Result<Vec<Vec<String>>> {
    let values = value.as_array().context("requires nonempty checks")?;
    ensure!(!values.is_empty(), "requires nonempty checks");
    values.iter().map(command).collect()
}

fn ignored_file(repo: &Path, path: &Path) -> Result<PathBuf> {
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

fn owned_path(value: &Value) -> Result<String> {
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
fn fingerprint_json(value: &Value) -> Result<String> {
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

fn load_graph(repo: &Path, path: &Path) -> Result<Graph> {
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
    let integration_checks = checks(&raw["integration_checks"]).context("graph requires integration_checks")?;
    let entries = raw["tickets"]
        .as_array()
        .context("graph requires nonempty tickets")?;
    ensure!(!entries.is_empty(), "graph requires nonempty tickets");
    let slug = Regex::new(r"^[a-z0-9][a-z0-9-]{0,31}$")?;
    let subject =
        Regex::new(r"^(feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert)(\([a-z0-9-]+\))?: [^\r\n]+$")?;
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

fn parse_ticket(
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
    let deliverable = text(&entry["deliverable"]).with_context(|| format!("{id} requires a deliverable"))?;
    ensure!(!deliverable.trim().is_empty(), "{id} requires a deliverable");
    let dependencies =
        strings(&entry["depends_on"]).with_context(|| format!("{id} requires depends_on as an ID list"))?;
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

fn validate_ticket_graph(tickets: &BTreeMap<String, Ticket>) -> Result<()> {
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

fn save_state(path: &Path, state: &State) -> Result<()> {
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().context("state has no parent")?)?;
    serde_json::to_writer_pretty(&mut temporary, state)?;
    temporary.write_all(b"\n")?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn load_state(path: &Path, graph: &Graph, head: String) -> Result<State> {
    if !path.exists() {
        return Ok(State {
            version: 1,
            graph_digest: graph.digest.clone(),
            integration_head: head,
            tasks: graph
                .tickets
                .keys()
                .map(|id| (id.clone(), json!({"status":"pending", "history":[]})))
                .collect(),
            adopted_heads: Vec::new(),
        });
    }

    let state: State = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(
        state.graph_digest == graph.digest,
        "graph or briefs changed; use a new graph for revised work"
    );
    ensure!(
        state.version == 1 && state.tasks.keys().eq(graph.tickets.keys()),
        "state does not match graph"
    );
    ensure!(
        state.tasks.values().all(|t| matches!(
            t["status"].as_str(),
            Some("pending" | "running" | "validated" | "merging" | "checking" | "integrated" | "blocked")
        ) && t["history"].is_array()),
        "invalid task status or history"
    );
    Ok(state)
}

fn task_state<'a>(state: &'a State, id: &str) -> Result<&'a Value> {
    state
        .tasks
        .get(id)
        .with_context(|| format!("state is missing ticket {id}"))
}

fn task_state_mut<'a>(state: &'a mut State, id: &str) -> Result<&'a mut Value> {
    state
        .tasks
        .get_mut(id)
        .with_context(|| format!("state is missing ticket {id}"))
}

fn frontier(graph: &Graph, state: &State) -> Vec<String> {
    graph
        .tickets
        .iter()
        .filter(|(id, ticket)| {
            state.tasks[*id]["status"] == "pending"
                && ticket
                    .dependencies
                    .iter()
                    .all(|d| state.tasks[d]["status"] == "integrated")
        })
        .map(|(id, _)| id.clone())
        .collect()
}

fn process_alive(pid: u32) -> Result<bool> {
    let output = Command::new("ps").args(["-axo", "pgid=,stat="]).output()?;
    ensure!(output.status.success(), "cannot inspect process groups");
    Ok(String::from_utf8_lossy(&output.stdout).lines().any(|row| {
        let mut fields = row.split_whitespace();
        fields.next().and_then(|v| v.parse::<u32>().ok()) == Some(pid)
            && fields.next().is_some_and(|s| !s.starts_with('Z'))
    }))
}

fn stop_process(child: &mut Child) -> Result<()> {
    let pid = child.id();
    if process_alive(pid)? {
        match signal::killpg(Pid::from_raw(pid as i32), Signal::SIGTERM) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
            Err(error) => {
                ensure!(!process_alive(pid)?, "cannot stop worker group: {error}");
            }
        }

        let deadline = Instant::now() + Duration::from_secs(3);
        while process_alive(pid)? && Instant::now() < deadline {
            if child.try_wait()?.is_some() {
                break;
            }

            thread::sleep(Duration::from_millis(20));
        }

        if process_alive(pid)? {
            match signal::killpg(Pid::from_raw(pid as i32), Signal::SIGKILL) {
                Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
                Err(error) => {
                    ensure!(!process_alive(pid)?, "cannot kill worker group: {error}");
                }
            }
        }
    }

    child.wait()?;
    Ok(())
}

fn watch_worker(child: &Mutex<Child>, cancelled: &AtomicBool, expired: &AtomicBool, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if cancelled.load(Ordering::SeqCst) {
            return Ok(());
        }

        let mut child = child
            .lock()
            .map_err(|_| anyhow!("worker process lock poisoned"))?;
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            expired.store(true, Ordering::SeqCst);
            stop_process(&mut child)?;
            return Ok(());
        }

        drop(child);
        thread::sleep(Duration::from_millis(20));
    }
}

fn settle_children(child: &mut Child) -> Result<bool> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while process_alive(child.id())? && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(30));
    }
    if process_alive(child.id())? {
        stop_process(child)?;
        return Ok(false);
    }

    Ok(true)
}

struct Worker {
    child: Arc<Mutex<Child>>,
    cancelled: Arc<AtomicBool>,
    expired: Arc<AtomicBool>,
    watch_error: Arc<Mutex<Option<String>>>,
    timer: Option<thread::JoinHandle<()>>,
}

impl Worker {
    fn spawn(mut cmd: Command, timeout: Duration) -> Result<Self> {
        let child = Arc::new(Mutex::new(cmd.process_group(0).spawn()?));
        let cancelled = Arc::new(AtomicBool::new(false));
        let expired = Arc::new(AtomicBool::new(false));
        let watch_error = Arc::new(Mutex::new(None));
        let (watch_child, watch_cancelled, watch_expired, watch_failure) =
            (child.clone(), cancelled.clone(), expired.clone(), watch_error.clone());
        let timer = thread::spawn(move || {
            let result = watch_worker(&watch_child, &watch_cancelled, &watch_expired, timeout);
            if let Err(error) = result
                && let Ok(mut failure) = watch_failure.lock()
            {
                *failure = Some(format!("{error:#}"));
            }
        });
        Ok(Self {
            child,
            cancelled,
            expired,
            watch_error,
            timer: Some(timer),
        })
    }

    fn child_process(&self) -> Result<MutexGuard<'_, Child>> {
        self.child
            .lock()
            .map_err(|_| anyhow!("worker process lock poisoned"))
    }

    fn check_watcher(&self) -> Result<()> {
        if let Some(error) = self
            .watch_error
            .lock()
            .map_err(|_| anyhow!("worker timeout watcher state poisoned"))?
            .take()
        {
            bail!("worker timeout watcher failed: {error}");
        }

        Ok(())
    }

    fn cancel_timer(&mut self) -> Result<()> {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(timer) = self.timer.take() {
            timer
                .join()
                .map_err(|_| anyhow!("worker timeout watcher panicked"))?;
        }

        self.check_watcher()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(timer) = self.timer.take() {
            self.cancelled.store(true, Ordering::SeqCst);
            if timer.join().is_err() {
                eprintln!("worker timeout watcher panicked during cleanup");
            }
        }

        let mut child = self
            .child
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Err(error) = stop_process(&mut child) {
            eprintln!("cannot stop worker during cleanup: {error:#}");
        }
    }
}

fn verify_integration(repo: &Path, graph: &Graph, state: &State) -> Result<()> {
    ensure!(
        git_text(repo, &["symbolic-ref", "HEAD"])? == graph.branch,
        "integration branch changed outside the dispatcher"
    );
    ensure!(
        commit(repo, "HEAD")? == state.integration_head,
        "integration HEAD changed outside the dispatcher"
    );
    ensure!(
        git(repo, &["status", "--porcelain", "--untracked-files=all"])?.is_empty(),
        "integration checkout changed; preserve and reconcile local edits"
    );
    Ok(())
}

fn verify_worker(repo: &Path, ticket: &Ticket, item: &Value) -> Result<(PathBuf, Vec<String>)> {
    let worker = PathBuf::from(text(&item["worker_path"])?);
    ensure!(
        git_text(&worker, &["symbolic-ref", "HEAD"])? == text(&item["worker_branch"])?,
        "worker branch changed"
    );
    ensure!(
        commit(&worker, "HEAD")? == text(&item["starting_commit"])?,
        "worker committed or changed HEAD; dispatcher owns Git transfer"
    );
    let mut changed = git(&worker, &["diff", "HEAD", "--name-only", "--no-renames", "-z"])?;
    changed.extend(git(&worker, &["ls-files", "--others", "--exclude-standard", "-z"])?);
    let paths: BTreeSet<_> = String::from_utf8(changed)?
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect();
    for path in &paths {
        ensure!(
            ticket
                .paths
                .iter()
                .any(|owned| path == owned || path.starts_with(&format!("{owned}/"))),
            "worker changes unowned path: {path}"
        );
    }

    ensure!(
        commit(repo, "HEAD")? == text(&item["expected_integration_head"])?,
        "integration HEAD changed outside the dispatcher"
    );
    Ok((worker, paths.into_iter().collect()))
}

fn run_checks(commands: &[Vec<String>], cwd: &Path, log_path: &Path, timeout: Duration) -> Result<Value> {
    let mut proof = Vec::new();
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    for argv in commands {
        ensure!(!interruption().load(Ordering::SeqCst), "received interruption signal");
        writeln!(log, "\nCHECK {}", serde_json::to_string(argv)?)?;
        log.flush()?;
        let mut child = Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log.try_clone()?)
            .process_group(0)
            .spawn()?;
        let deadline = Instant::now() + timeout;
        let status = loop {
            if interruption().load(Ordering::SeqCst) || Instant::now() >= deadline {
                stop_process(&mut child)?;
                bail!("check timed out or interrupted; see {}", log_path.display());
            }

            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(error) => {
                    stop_process(&mut child)?;
                    return Err(error.into());
                }
            }
        };
        let settled = settle_children(&mut child)?;
        let code = status.code().unwrap_or(-1);
        proof.push(json!({"command":argv,"exit_code":code,"log":log_path}));
        ensure!(status.success(), "check failed ({code}); see {}", log_path.display());
        ensure!(settled, "check left live child processes; see {}", log_path.display());
    }

    Ok(json!(proof))
}

fn merge_matches(repo: &Path, item: &Value) -> Result<bool> {
    let row = git_text(repo, &["rev-list", "--parents", "-n", "1", "HEAD"])?;
    let parents: Vec<_> = row.split_whitespace().skip(1).collect();
    Ok(parents == [text(&item["merge_base"])?, text(&item["result_commit"])?])
}

fn integrate(repo: &Path, graph: &Graph, id: &str, state: &mut State, path: &Path, timeout: Duration) -> Result<()> {
    if state.tasks[id]["status"] == "validated" {
        verify_integration(repo, graph, state)?;
        task_state_mut(state, id)?["status"] = json!("merging");
        task_state_mut(state, id)?["merge_base"] = json!(state.integration_head);
        save_state(path, state)?;
        if git(
            repo,
            &[
                "merge",
                "--no-ff",
                "--no-edit",
                text(&state.tasks[id]["result_commit"])?,
            ],
        )
        .is_err()
        {
            let merge_head = git_text(
                repo,
                &["rev-parse", "--path-format=absolute", "--git-path", "MERGE_HEAD"],
            )?;
            if Path::new(&merge_head).exists() {
                git(repo, &["merge", "--abort"])?;
            }

            bail!("merge failed for {id}; worker result retained");
        }
    }

    if state.tasks[id]["status"] == "merging" {
        ensure!(
            merge_matches(repo, &state.tasks[id])?,
            "cannot reconcile interrupted merge; inspect Git before resuming"
        );
        let head = commit(repo, "HEAD")?;
        task_state_mut(state, id)?["status"] = json!("checking");
        task_state_mut(state, id)?["integration_commit"] = json!(head);
        state.integration_head = head;
        save_state(path, state)?;
    }

    if state.tasks[id]["status"] == "checking" {
        verify_integration(repo, graph, state)?;
        let proof = run_checks(
            &graph.checks,
            repo,
            &path
                .parent()
                .context("dispatch state has no parent")?
                .join("integration.log"),
            timeout,
        )?;
        verify_integration(repo, graph, state)?;
        task_state_mut(state, id)?["status"] = json!("integrated");
        task_state_mut(state, id)?["integration_checks"] = proof;
        save_state(path, state)?;
        println!(
            "{}",
            json!({"event":"integrated","task":id,"commit":state.integration_head})
        );
    }

    Ok(())
}

struct Execution<'a> {
    repo: &'a Path,
    graph: &'a Graph,
    jobs: usize,
    worker_timeout: Duration,
    check_timeout: Duration,
    retry: &'a [String],
    accept_head: Option<&'a str>,
}

struct RunState {
    state_path: PathBuf,
    state: State,
    active: BTreeMap<String, Worker>,
    current: Option<String>,
}

impl RunState {
    fn load(repo: &Path, graph: &Graph, directory: &Path) -> Result<Self> {
        let state_path = directory.join("state.json");
        let state = load_state(&state_path, graph, commit(repo, "HEAD")?)?;
        Ok(Self {
            state_path,
            state,
            active: BTreeMap::new(),
            current: None,
        })
    }
}

impl Execution<'_> {
    fn execute(&self, directory: &Path) -> Result<()> {
        let mut run = RunState::load(self.repo, self.graph, directory)?;
        let result = self.run(&mut run, directory);
        if let Err(error) = &result {
            self.preserve_failed_run(&mut run, error);
        }

        result
    }

    fn run(&self, run: &mut RunState, directory: &Path) -> Result<()> {
        self.reconcile_merges(run)?;
        self.prepare_run(run)?;
        self.verify_adopted_heads(run, directory)?;
        self.integrate_validated(run)?;
        self.execute_tickets(run, directory)?;
        println!(
            "{}",
            json!({"event":"complete","integration_head":run.state.integration_head})
        );
        Ok(())
    }

    fn reconcile_merges(&self, run: &mut RunState) -> Result<()> {
        let merging: Vec<_> = self
            .graph
            .tickets
            .keys()
            .filter(|id| run.state.tasks[*id]["status"] == "merging")
            .cloned()
            .collect();
        for id in merging {
            run.current = Some(id.clone());
            if commit(self.repo, "HEAD")? == text(&task_state(&run.state, &id)?["merge_base"])? {
                task_state_mut(&mut run.state, &id)?["status"] = json!("validated");
            } else {
                integrate(
                    self.repo,
                    self.graph,
                    &id,
                    &mut run.state,
                    &run.state_path,
                    self.check_timeout,
                )?;
            }
        }

        Ok(())
    }

    fn prepare_run(&self, run: &mut RunState) -> Result<()> {
        run.current = None;
        for item in run.state.tasks.values() {
            if matches!(item["status"].as_str(), Some("running" | "blocked"))
                && let Some(pid) = item["pid"].as_u64()
            {
                ensure!(
                    !process_alive(pid as u32)?,
                    "a previous worker group is still live; stop it before resuming"
                );
            }
        }

        let head = commit(self.repo, "HEAD")?;
        if head != run.state.integration_head {
            ensure!(
                self.accept_head
                    .is_some_and(|reference| commit(self.repo, reference).is_ok_and(|accepted| accepted == head))
                    && ancestor(self.repo, &run.state.integration_head, &head)?,
                "integration HEAD drifted; inspect and explicitly --accept-head <commit>"
            );
            run.state
                .adopted_heads
                .push(json!({"previous":run.state.integration_head,"accepted":head,"status":"checking"}));
            run.state.integration_head = head;
        }

        for id in self.retry {
            let item = task_state_mut(&mut run.state, id)
                .context("--retry requires a blocked or interrupted running ticket")?;
            ensure!(
                matches!(item["status"].as_str(), Some("blocked" | "running")),
                "--retry requires a blocked or interrupted running ticket"
            );
            let mut attempt = item.clone();
            attempt
                .as_object_mut()
                .context("invalid task state")?
                .remove("history");
            item["history"]
                .as_array_mut()
                .context("invalid attempt history")?
                .push(attempt);
            if item.get("integration_commit").is_some() {
                item["status"] = json!("checking");
            } else {
                let history = item["history"].take();
                *item = json!({"status":"pending","history":history});
            }
        }

        ensure!(
            !run.state
                .tasks
                .values()
                .any(|item| matches!(item["status"].as_str(), Some("blocked" | "running"))),
            "blocked or interrupted tickets require inspection and explicit --retry <id>"
        );
        save_state(&run.state_path, &run.state)
    }

    fn verify_adopted_heads(&self, run: &mut RunState, directory: &Path) -> Result<()> {
        for index in 0..run.state.adopted_heads.len() {
            if run.state.adopted_heads[index]["status"] != "checking" {
                continue;
            }

            verify_integration(self.repo, self.graph, &run.state)?;
            let proof = run_checks(
                &self.graph.checks,
                self.repo,
                &directory.join("integration.log"),
                self.check_timeout,
            )?;
            verify_integration(self.repo, self.graph, &run.state)?;
            run.state.adopted_heads[index]["status"] = json!("verified");
            run.state.adopted_heads[index]["checks"] = proof;
            save_state(&run.state_path, &run.state)?;
        }

        Ok(())
    }

    fn integrate_validated(&self, run: &mut RunState) -> Result<()> {
        for id in self.graph.tickets.keys() {
            if !matches!(
                task_state(&run.state, id)?["status"].as_str(),
                Some("validated" | "checking")
            ) {
                continue;
            }

            run.current = Some(id.clone());
            integrate(
                self.repo,
                self.graph,
                id,
                &mut run.state,
                &run.state_path,
                self.check_timeout,
            )?;
        }

        Ok(())
    }

    fn execute_tickets(&self, run: &mut RunState, directory: &Path) -> Result<()> {
        while run
            .state
            .tasks
            .values()
            .any(|item| item["status"] != "integrated")
        {
            ensure!(!interruption().load(Ordering::SeqCst), "received interruption signal");
            self.check_active_workers(run)?;
            self.start_ready_workers(run, directory)?;
            ensure!(!run.active.is_empty(), "no runnable tickets; inspect graph and state");
            let completed = self.completed_workers(run)?;
            if completed.is_empty() {
                run.current = None;
                thread::sleep(Duration::from_millis(50));
                continue;
            }

            for id in completed {
                run.current = Some(id.clone());
                self.finish_worker(run, &id)?;
            }
        }

        Ok(())
    }

    fn check_active_workers(&self, run: &mut RunState) -> Result<()> {
        for (id, worker) in &run.active {
            run.current = Some(id.clone());
            worker.check_watcher()?;
            ensure!(!worker.expired.load(Ordering::SeqCst), "worker timed out: {id}");
            let status = worker.child_process()?.try_wait()?;
            let Some(status) = status else {
                continue;
            };
            ensure!(!worker.expired.load(Ordering::SeqCst), "worker timed out: {id}");
            ensure!(
                status.success(),
                "worker failed ({}); see {}",
                status.code().unwrap_or(-1),
                text(&task_state(&run.state, id)?["worker_log"])?
            );
        }

        Ok(())
    }

    fn start_ready_workers(&self, run: &mut RunState, directory: &Path) -> Result<()> {
        for id in frontier(self.graph, &run.state) {
            if run.active.len() >= self.jobs {
                break;
            }
            if worker_paths_overlap(self.graph, &id, &run.active) {
                continue;
            }

            run.current = Some(id.clone());
            verify_integration(self.repo, self.graph, &run.state)?;
            let worker = self.start_worker(run, &id, directory)?;
            run.active.insert(id, worker);
        }

        Ok(())
    }

    fn start_worker(&self, run: &mut RunState, id: &str, directory: &Path) -> Result<Worker> {
        let ticket = self
            .graph
            .tickets
            .get(id)
            .context("graph is missing a runnable ticket")?;
        let history = task_state(&run.state, id)?["history"]
            .as_array()
            .context("task history is invalid")?;
        let attempt = history.len() + 1;
        let hash = format!("{:x}", Sha256::digest(self.graph.path.as_os_str().as_encoded_bytes()));
        let worker_root = self
            .repo
            .parent()
            .context("repository has no parent")?
            .join(format!(
                ".{}-dispatch-{}",
                self.repo
                    .file_name()
                    .context("repository has no name")?
                    .to_string_lossy(),
                &hash[..12]
            ));
        let worker_path = worker_root.join(format!("{id}-{attempt}"));
        let branch = format!("{}-{id}-{attempt}", self.graph.branch.trim_start_matches("refs/heads/"));
        let head = commit(self.repo, "HEAD")?;
        let item = task_state_mut(&mut run.state, id)?;
        item["status"] = json!("running");
        item["starting_commit"] = json!(head);
        item["worker_path"] = json!(worker_path);
        item["worker_branch"] = json!(format!("refs/heads/{branch}"));
        item["pid"] = Value::Null;
        save_state(&run.state_path, &run.state)?;
        git(
            self.repo,
            &[
                "worktree",
                "add",
                "-b",
                &branch,
                worker_path.to_str().context("non-UTF8 worker path")?,
                &head,
            ],
        )?;
        let packet = write_worker_packet(self.graph, &worker_path, id, &head, ticket)?;
        let log_path = directory.join(format!("{id}-{attempt}.log"));
        let log = File::create(&log_path)?;
        let mut command = Command::new(&ticket.worker[0]);
        command
            .args(&ticket.worker[1..])
            .current_dir(&worker_path)
            .stdin(File::open(&packet)?)
            .stdout(log.try_clone()?)
            .stderr(log);
        let worker = Worker::spawn(command, self.worker_timeout)?;
        let pid = worker.child_process()?.id();
        let item = task_state_mut(&mut run.state, id)?;
        item["pid"] = json!(pid);
        item["worker_command"] = json!(ticket.worker);
        item["worker_log"] = json!(log_path);
        save_state(&run.state_path, &run.state)?;
        println!("{}", json!({"event":"started","task":id,"worktree":worker_path}));
        Ok(worker)
    }

    fn completed_workers(&self, run: &mut RunState) -> Result<Vec<String>> {
        let mut completed = Vec::new();
        for (id, worker) in &run.active {
            run.current = Some(id.clone());
            worker.check_watcher()?;
            ensure!(!worker.expired.load(Ordering::SeqCst), "worker timed out: {id}");
            if worker.child_process()?.try_wait()?.is_some() {
                completed.push(id.clone());
            }
        }

        Ok(completed)
    }

    fn finish_worker(&self, run: &mut RunState, id: &str) -> Result<()> {
        let worker = run
            .active
            .get_mut(id)
            .context("completed worker is missing")?;
        worker.cancel_timer()?;
        let mut child = worker.child_process()?;
        let settled = settle_children(&mut child)?;
        ensure!(!worker.expired.load(Ordering::SeqCst), "worker timed out: {id}");
        let status = child.wait()?;
        ensure!(
            status.success(),
            "worker failed ({}); see {}",
            status.code().unwrap_or(-1),
            text(&task_state(&run.state, id)?["worker_log"])?
        );
        ensure!(settled, "worker left live child processes");
        drop(child);
        run.active.remove(id);
        task_state_mut(&mut run.state, id)?["expected_integration_head"] = json!(run.state.integration_head);
        let ticket = self
            .graph
            .tickets
            .get(id)
            .context("graph is missing a completed ticket")?;
        let item = task_state(&run.state, id)?;
        let (worker_path, _) = verify_worker(self.repo, ticket, item)?;
        let proof = run_checks(
            &ticket.checks,
            &worker_path,
            Path::new(text(&task_state(&run.state, id)?["worker_log"])?),
            self.check_timeout,
        )?;
        let (_, paths) = verify_worker(self.repo, ticket, task_state(&run.state, id)?)?;
        ensure!(
            !paths.is_empty(),
            "ticket produced no changes; inspect acceptance and task scope"
        );
        git(&worker_path, &["add", "--all"])?;
        git(&worker_path, &["commit", "-m", &ticket.message])?;
        let result_commit = commit(&worker_path, "HEAD")?;
        let item = task_state_mut(&mut run.state, id)?;
        item["status"] = json!("validated");
        item["result_commit"] = json!(result_commit);
        item["worker_checks"] = proof;
        save_state(&run.state_path, &run.state)?;
        integrate(
            self.repo,
            self.graph,
            id,
            &mut run.state,
            &run.state_path,
            self.check_timeout,
        )
    }

    fn preserve_failed_run(&self, run: &mut RunState, error: &anyhow::Error) {
        let mut cleanup_errors = Vec::new();
        if let Some(id) = &run.current {
            let current_status = match task_state(&run.state, id) {
                Ok(item) => item["status"].as_str().map(str::to_owned),
                Err(error) => {
                    cleanup_errors.push(format!("cannot inspect current ticket {id}: {error:#}"));
                    None
                }
            };
            if let Some(status) = current_status
                && status != "integrated"
            {
                let completed_merge = status == "merging"
                    && task_state(&run.state, id).is_ok_and(|item| matches!(merge_matches(self.repo, item), Ok(true)));
                let next_status = if completed_merge { "merging" } else { "blocked" };
                match task_state_mut(&mut run.state, id) {
                    Ok(item) => {
                        item["status"] = json!(next_status);
                        item["reason"] = json!(format!("{error:#}"));
                    }
                    Err(error) => cleanup_errors.push(format!("cannot block current ticket {id}: {error:#}")),
                }
            }
        }

        for (id, worker) in &mut run.active {
            if let Err(error) = worker.cancel_timer() {
                cleanup_errors.push(format!("cannot stop timeout watcher for {id}: {error:#}"));
            }
            match worker.child_process() {
                Ok(mut child) => {
                    if let Err(error) = stop_process(&mut child) {
                        cleanup_errors.push(format!("cannot stop worker {id}: {error:#}"));
                    }
                }
                Err(error) => cleanup_errors.push(format!("cannot access worker {id}: {error:#}")),
            }
            match task_state_mut(&mut run.state, id) {
                Ok(item) => {
                    item["status"] = json!("blocked");
                    if run.current.as_ref() != Some(id) {
                        item["reason"] = json!("run stopped; inspect retained work before retry");
                    }
                }
                Err(error) => cleanup_errors.push(format!("cannot block active ticket {id}: {error:#}")),
            }
        }

        if let Err(error) = save_state(&run.state_path, &run.state) {
            cleanup_errors.push(format!("cannot save failure state: {error:#}"));
        }
        for error in cleanup_errors {
            eprintln!("ERROR: failure cleanup: {error}");
        }
    }
}

fn write_worker_packet(graph: &Graph, worker_path: &Path, id: &str, head: &str, ticket: &Ticket) -> Result<PathBuf> {
    let work_item = graph
        .path
        .parent()
        .context("graph has no parent")?
        .file_name()
        .context("graph parent has no name")?;
    let packet = worker_path
        .join(".agent/work")
        .join(work_item)
        .join("task.md");
    git(
        worker_path,
        &[
            "check-ignore",
            "-q",
            "--",
            packet.to_str().context("non-UTF8 packet path")?,
        ],
    )?;
    fs::create_dir_all(packet.parent().context("task packet has no parent")?)?;
    fs::write(
        &packet,
        format!(
            "Implement only ticket {id}: {}\nWorktree: {}\nStarting commit: {head}\nOwned paths: {}\nRead actual source and repository instructions. Keep all edits in this worktree.\nDo not commit, switch branches, integrate other work, publish, or spawn another orchestration team.\nThe dispatcher owns commits, checks, and integration. Report remaining risks.\n\n{}",
            ticket.deliverable,
            worker_path.display(),
            serde_json::to_string(&ticket.paths)?,
            ticket.brief
        ),
    )?;
    Ok(packet)
}

fn worker_paths_overlap(graph: &Graph, id: &str, active: &BTreeMap<String, Worker>) -> bool {
    let ticket = &graph.tickets[id];
    active.keys().any(|other| {
        ticket.paths.iter().any(|path| {
            graph.tickets[other].paths.iter().any(|owned| {
                path == owned || path.starts_with(&format!("{owned}/")) || owned.starts_with(&format!("{path}/"))
            })
        })
    })
}

/// Run the dispatcher with argv including the executable name.
///
/// # Errors
///
/// Returns an error when arguments, the repository, the graph, persisted state,
/// or any worker or integration operation is invalid or fails.
pub fn run(args: Vec<OsString>) -> Result<()> {
    let args = match Arguments::try_parse_from(args) {
        Ok(args) => args,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            error.print()?;
            return Ok(());
        }
        Err(error) => return Err(anyhow!(error.to_string())),
    };
    let repo = args.repo.canonicalize()?;
    ensure!(
        Path::new(&git_text(&repo, &["rev-parse", "--show-toplevel"])?).canonicalize()? == repo,
        "--repo must name the integration worktree root"
    );
    let graph = load_graph(&repo, &args.graph)?;
    let directory = graph
        .path
        .parent()
        .context("graph has no parent")?
        .join("dispatch");
    match args.command {
        Action::Status => {
            let head = commit(&repo, "HEAD")?;
            let state = load_state(&directory.join("state.json"), &graph, head.clone())?;
            let mut output = serde_json::to_value(&state)?;
            output["ready"] = json!(frontier(&graph, &state));
            output["observed_head"] = json!(head);
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        Action::Run {
            jobs,
            worker_timeout,
            check_timeout,
            retry,
            accept_head,
        } => {
            ensure!(
                jobs > 0
                    && [worker_timeout, check_timeout]
                        .iter()
                        .all(|n| n.is_finite() && *n > 0.0),
                "jobs and timeouts must be positive"
            );
            let worker_timeout = Duration::try_from_secs_f64(worker_timeout).context("invalid worker timeout")?;
            let check_timeout = Duration::try_from_secs_f64(check_timeout).context("invalid check timeout")?;
            ensure!(
                git(&repo, &["status", "--porcelain", "--untracked-files=all"])?.is_empty(),
                "integration worktree must be clean before run"
            );
            fs::create_dir_all(&directory)?;
            let lock = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(repo.join(".agent/work/.dispatch.lock"))?;
            FileExt::try_lock_exclusive(&lock).context("dispatcher is already running")?;
            interruption().store(false, Ordering::SeqCst);
            let term = signal_hook::flag::register(signal_hook::consts::SIGTERM, interruption().clone())?;
            let int = signal_hook::flag::register(signal_hook::consts::SIGINT, interruption().clone())?;
            let result = Execution {
                repo: &repo,
                graph: &graph,
                jobs,
                worker_timeout,
                check_timeout,
                retry: &retry,
                accept_head: accept_head.as_deref(),
            }
            .execute(&directory);
            signal_hook::low_level::unregister(term);
            signal_hook::low_level::unregister(int);

            result?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Execution, Graph, RunState, State, Worker, fingerprint_json, process_alive};
    use anyhow::{Result, anyhow, ensure};
    use serde_json::json;
    use std::{collections::BTreeMap, fs, process::Command, time::Duration};

    #[test]
    fn legacy_fingerprints_preserve_sorted_ascii_json() {
        assert_eq!(
            fingerprint_json(&json!({"z":"é😀\u{7f}","a":[true,1,null,"line\n"]})).unwrap(),
            r#"{"a": [true, 1, null, "line\n"], "z": "\u00e9\ud83d\ude00\u007f"}"#
        );
        assert_eq!(
            fingerprint_json(&json!([1e-7, 1e16, 0.0001, 10.0, -0.0])).unwrap(),
            "[1e-07, 1e+16, 0.0001, 10.0, -0.0]"
        );
    }

    #[test]
    fn failure_cleanup_persists_primary_error_when_watcher_fails() -> Result<()> {
        let root = tempfile::tempdir()?;
        let graph = Graph {
            path: root.path().join("tickets.json"),
            branch: "refs/heads/main".to_owned(),
            checks: Vec::new(),
            tickets: BTreeMap::new(),
            digest: "graph".to_owned(),
        };
        let mut command = Command::new("sleep");
        command.arg("30");
        let worker = Worker::spawn(command, Duration::from_secs(60))?;
        let pid = worker.child_process()?.id();
        *worker
            .watch_error
            .lock()
            .map_err(|_| anyhow!("worker timeout watcher state poisoned"))? =
            Some("simulated watcher failure".to_owned());
        let mut tasks = BTreeMap::new();
        tasks.insert("ticket".to_owned(), json!({"status":"running", "history":[]}));
        let mut active = BTreeMap::new();
        active.insert("ticket".to_owned(), worker);
        let state_path = root.path().join("state.json");
        let mut run = RunState {
            state_path: state_path.clone(),
            state: State {
                version: 1,
                graph_digest: "graph".to_owned(),
                integration_head: "head".to_owned(),
                tasks,
                adopted_heads: Vec::new(),
            },
            active,
            current: Some("ticket".to_owned()),
        };
        let execution = Execution {
            repo: root.path(),
            graph: &graph,
            jobs: 1,
            worker_timeout: Duration::from_secs(60),
            check_timeout: Duration::from_secs(60),
            retry: &[],
            accept_head: None,
        };

        execution.preserve_failed_run(&mut run, &anyhow!("primary failure"));

        ensure!(run.state.tasks["ticket"]["status"] == "blocked");
        ensure!(run.state.tasks["ticket"]["reason"] == "primary failure");
        let saved: State = serde_json::from_slice(&fs::read(state_path)?)?;
        ensure!(saved.tasks["ticket"]["status"] == "blocked");
        ensure!(saved.tasks["ticket"]["reason"] == "primary failure");
        ensure!(!process_alive(pid)?);
        Ok(())
    }
}
