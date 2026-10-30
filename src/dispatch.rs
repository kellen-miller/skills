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

mod execution;
mod graph;
mod worker;

use execution::Execution;
use graph::{load_graph, text};
use worker::{Worker, process_alive, settle_children, stop_process};

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
    git_text(
        repo,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
    )
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

fn save_state(path: &Path, state: &State) -> Result<()> {
    let mut temporary =
        tempfile::NamedTempFile::new_in(path.parent().context("state has no parent")?)?;
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
            Some(
                "pending"
                    | "running"
                    | "validated"
                    | "merging"
                    | "checking"
                    | "integrated"
                    | "blocked"
            )
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
    let mut changed = git(
        &worker,
        &["diff", "HEAD", "--name-only", "--no-renames", "-z"],
    )?;
    changed.extend(git(
        &worker,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?);
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

fn run_checks(
    commands: &[Vec<String>],
    cwd: &Path,
    log_path: &Path,
    timeout: Duration,
) -> Result<Value> {
    let mut proof = Vec::new();
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    for argv in commands {
        ensure!(
            !interruption().load(Ordering::SeqCst),
            "received interruption signal"
        );
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
        ensure!(
            status.success(),
            "check failed ({code}); see {}",
            log_path.display()
        );
        ensure!(
            settled,
            "check left live child processes; see {}",
            log_path.display()
        );
    }

    Ok(json!(proof))
}

fn merge_matches(repo: &Path, item: &Value) -> Result<bool> {
    let row = git_text(repo, &["rev-list", "--parents", "-n", "1", "HEAD"])?;
    let parents: Vec<_> = row.split_whitespace().skip(1).collect();
    Ok(parents == [text(&item["merge_base"])?, text(&item["result_commit"])?])
}

fn integrate(
    repo: &Path,
    graph: &Graph,
    id: &str,
    state: &mut State,
    path: &Path,
    timeout: Duration,
) -> Result<()> {
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
                &[
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-path",
                    "MERGE_HEAD",
                ],
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
            let worker_timeout =
                Duration::try_from_secs_f64(worker_timeout).context("invalid worker timeout")?;
            let check_timeout =
                Duration::try_from_secs_f64(check_timeout).context("invalid check timeout")?;
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
            let term =
                signal_hook::flag::register(signal_hook::consts::SIGTERM, interruption().clone())?;
            let int =
                signal_hook::flag::register(signal_hook::consts::SIGINT, interruption().clone())?;
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
    use super::execution::{Execution, RunState};
    use super::graph::fingerprint_json;
    use super::worker::{Worker, process_alive};
    use super::{Graph, State};
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
        tasks.insert(
            "ticket".to_owned(),
            json!({"status":"running", "history":[]}),
        );
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
