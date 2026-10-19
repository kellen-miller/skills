use fs2::FileExt;
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::OnceLock,
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

fn worker_binary() -> &'static Path {
    static FIXTURE: OnceLock<(TempDir, PathBuf)> = OnceLock::new();
    &FIXTURE
        .get_or_init(|| {
            let directory = tempfile::tempdir().unwrap();
            let binary = directory.path().join("worker");
            let result = Command::new("rustc")
                .args(["--edition=2024", "tests/fixtures/dispatch_worker.rs", "-o"])
                .arg(&binary)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            (directory, binary)
        })
        .1
}

struct Fixture {
    root: TempDir,
    repo: PathBuf,
    work: PathBuf,
    graph: PathBuf,
    initial: String,
    data: Value,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir(&repo).unwrap();
        let work = repo.join(".agent/work/feature");
        let mut fixture = Self {
            root,
            repo,
            graph: work.join("tickets.json"),
            work,
            initial: String::new(),
            data: json!({
                "version":1,"approved":true,"base_ref":"refs/remotes/origin/main","integration_branch":"refs/heads/main",
                "worker_command":[worker_binary(),"ok","a"],"integration_checks":[[worker_binary(),"check-exists","README.md"]],"tickets":[]
            }),
        };
        fixture.git(&["init", "-b", "main"]);
        fixture.git(&["config", "user.name", "Test User"]);
        fixture.git(&["config", "user.email", "test@example.com"]);
        fixture.git(&["config", "commit.gpgsign", "false"]);
        fixture.git(&["config", "core.hooksPath", "/dev/null"]);
        fs::write(fixture.repo.join(".gitignore"), ".agent/work/\n").unwrap();
        fs::write(fixture.repo.join("README.md"), "initial\n").unwrap();
        fixture.git(&["add", "."]);
        fixture.git(&["commit", "-m", "initial"]);
        fixture.initial = fixture.git(&["rev-parse", "HEAD"]);
        fixture.git(&["update-ref", "refs/remotes/origin/main", &fixture.initial]);
        fs::create_dir_all(&fixture.work).unwrap();
        fixture.add_ticket("a", &[], "ok", &["a.txt"]);
        fixture
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn add_ticket(&mut self, id: &str, dependencies: &[&str], mode: &str, paths: &[&str]) {
        fs::write(
            self.work.join(format!("{id}.md")),
            format!("Deliver {id}. Test its behavior.\n"),
        )
        .unwrap();
        self.data["tickets"].as_array_mut().unwrap().push(json!({
            "id":id,"deliverable":format!("deliver {id}"),"commit_message":format!("feat: implement {id}"),"brief":format!("{id}.md"),
            "depends_on":dependencies,"owned_paths":paths,"worker_command":[worker_binary(),mode,id],
            "checks":[[worker_binary(),"check-exists",format!("{id}.txt")]]
        }));
    }

    fn mode(&mut self, index: usize, mode: &str, extra: &[&Path]) {
        let id = self.data["tickets"][index]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        self.data["tickets"][index]["worker_command"] = json!([worker_binary(), mode, id]);
        for path in extra {
            self.data["tickets"][index]["worker_command"]
                .as_array_mut()
                .unwrap()
                .push(json!(path));
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        fs::write(&self.graph, serde_json::to_vec(&self.data).unwrap()).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_dispatch-tickets"));
        command
            .arg("--repo")
            .arg(&self.repo)
            .arg("--graph")
            .arg(&self.graph)
            .args(args);
        command
    }

    fn call(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    fn state(&self) -> Value {
        serde_json::from_slice(&fs::read(self.work.join("dispatch/state.json")).unwrap()).unwrap()
    }

    fn save_state(&self, state: &Value) {
        fs::write(
            self.work.join("dispatch/state.json"),
            serde_json::to_vec(state).unwrap(),
        )
        .unwrap();
    }
}

fn passed(output: Output) -> Output {
    assert!(
        output.status.success(),
        "stderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    output
}

fn failed(output: Output, message: &str) {
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "wanted {message:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn stopped(pid: &Path) {
    let result = Command::new("ps")
        .args(["-p", &fs::read_to_string(pid).unwrap(), "-o", "stat="])
        .output()
        .unwrap();
    let status = String::from_utf8_lossy(&result.stdout);
    assert!(
        status.trim().is_empty() || status.trim().starts_with('Z'),
        "child remains live: {status}"
    );
}

#[test]
fn status_is_read_only_and_reports_dependency_frontier() {
    let mut f = Fixture::new();
    f.add_ticket("b", &["a"], "ok", &["b.txt"]);
    f.add_ticket("c", &[], "ok", &["c.txt"]);
    let output = passed(f.call(&["status"]));
    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["ready"], json!(["a", "c"]));
    assert!(!f.work.join("dispatch").exists());
    assert_eq!(f.git(&["status", "--porcelain"]), "");
}

#[test]
fn parallel_workers_and_dependency_start_from_integrated_tip() {
    let mut f = Fixture::new();
    let root = f.root.path().to_owned();
    f.mode(0, "barrier", &[&root]);
    f.add_ticket("b", &[], "barrier", &["b.txt"]);
    f.mode(1, "barrier", &[&root]);
    f.add_ticket("c", &["a"], "need-a", &["c.txt"]);
    passed(f.call(&["run", "--jobs", "2"]));
    let state = f.state();
    for task in state["tasks"].as_object().unwrap().values() {
        assert_eq!(task["status"], "integrated");
        assert_eq!(task["worker_checks"][0]["exit_code"], 0);
        assert_eq!(task["integration_checks"][0]["exit_code"], 0);
        assert!(Path::new(task["worker_log"].as_str().unwrap()).exists());
    }

    assert_eq!(state["tasks"]["a"]["starting_commit"], f.initial);
    assert_eq!(state["tasks"]["b"]["starting_commit"], f.initial);
    assert_ne!(state["tasks"]["c"]["starting_commit"], f.initial);
    assert!(!Path::new(state["tasks"]["a"]["worker_path"].as_str().unwrap()).starts_with(&f.repo));
    for id in ["a", "b", "c"] {
        assert!(f.repo.join(format!("{id}.txt")).exists());
    }

    let head = f.git(&["rev-parse", "HEAD"]);
    passed(f.call(&["run", "--jobs", "2"]));
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
    failed(
        f.call(&["run", "--jobs", "2", "--retry", "a"]),
        "--retry requires",
    );
}

#[test]
fn overlapping_ownership_serializes_independent_tickets() {
    let mut f = Fixture::new();
    f.data["tickets"][0]["owned_paths"] = json!(["a.txt", "b.txt"]);
    f.add_ticket("b", &[], "ok", &["a.txt", "b.txt"]);
    passed(f.call(&["run", "--jobs", "2"]));
    let state = f.state();
    assert_eq!(
        state["tasks"]["b"]["starting_commit"],
        state["tasks"]["a"]["integration_commit"]
    );
}

#[test]
fn rejects_invalid_graphs_before_mutation() {
    let cases = [
        ("depends_on", json!(["a"]), "cycle"),
        ("depends_on", json!(["missing"]), "unknown"),
        ("depends_on", json!(["a", "a"]), "duplicate"),
        ("owned_paths", json!(["../outside"]), "unsafe"),
        ("owned_paths", json!(["/absolute"]), "unsafe"),
        ("owned_paths", json!([".git/config"]), "unsafe"),
        ("owned_paths", json!([".agent/work"]), "unsafe"),
        ("owned_paths", json!(["src/*"]), "unsafe"),
        ("owned_paths", json!(["src\\file"]), "unsafe"),
        ("worker_command", json!("shell command"), "argv arrays"),
        ("checks", json!([]), "checks"),
        (
            "commit_message",
            json!("bad message"),
            "Conventional Commit",
        ),
        ("deliverable", json!(" "), "deliverable"),
        ("id", json!("Upper"), "lowercase slugs"),
    ];
    for (field, value, error) in cases {
        let mut f = Fixture::new();
        f.data["tickets"][0][field] = value;
        failed(f.call(&["run", "--jobs", "1"]), error);
        assert!(!f.work.join("dispatch").exists());
        assert_eq!(f.git(&["rev-parse", "HEAD"]), f.initial);
    }
}

#[test]
fn explicit_approved_refs_ignored_inputs_and_ancestry_are_required() {
    for (field, value, error) in [
        ("approved", json!(false), "approved=true"),
        ("version", json!(2), "version=1"),
        ("base_ref", json!("main"), "explicit Git refs"),
        ("integration_branch", json!("main"), "explicit Git refs"),
        (
            "integration_branch",
            json!("refs/heads/other"),
            "integration branch",
        ),
    ] {
        let mut f = Fixture::new();
        f.data[field] = value;
        failed(f.call(&["status"]), error);
    }

    let f = Fixture::new();
    fs::write(f.repo.join(".gitignore"), "").unwrap();
    failed(f.call(&["status"]), "Git ignored");
    let mut f = Fixture::new();
    fs::write(f.repo.join("brief.md"), "outside").unwrap();
    f.data["tickets"][0]["brief"] = json!("../../../brief.md");
    failed(f.call(&["status"]), "under .agent/work");
    let f = Fixture::new();
    f.git(&["checkout", "--orphan", "unrelated"]);
    f.git(&["commit", "-m", "unrelated"]);
    f.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
    f.git(&["checkout", "main"]);
    failed(f.call(&["status"]), "does not descend");
}

#[test]
fn worker_staged_deletions_are_integrated() {
    let mut f = Fixture::new();
    f.mode(0, "git-rm", &[]);
    f.data["tickets"][0]["owned_paths"] = json!(["a.txt", "README.md"]);
    f.data["integration_checks"] = json!([[worker_binary(), "check-exists", "a.txt"]]);
    passed(f.call(&["run", "--jobs", "1"]));
    assert_eq!(f.state()["tasks"]["a"]["status"], "integrated");
    assert!(!f.repo.join("README.md").exists());
    assert_eq!(f.git(&["ls-files", "README.md"]), "");
}

#[test]
fn unowned_changes_retained_and_never_integrated() {
    let mut f = Fixture::new();
    f.mode(0, "outside", &[]);
    failed(f.call(&["run", "--jobs", "1"]), "unowned path");
    let state = f.state();
    let item = &state["tasks"]["a"];
    assert_eq!(item["status"], "blocked");
    assert!(
        Path::new(item["worker_path"].as_str().unwrap())
            .join("outside.txt")
            .exists()
    );
    assert!(!f.repo.join("outside.txt").exists());
    assert_eq!(f.git(&["rev-parse", "HEAD"]), f.initial);
}

#[test]
fn failed_worker_requires_explicit_retry_and_preserves_attempt() {
    let mut f = Fixture::new();
    let gate = f.root.path().join("gate");
    f.mode(0, "fail", &[&gate]);
    failed(f.call(&["run", "--jobs", "1"]), "worker failed (7)");
    let first = f.state()["tasks"]["a"]["worker_path"]
        .as_str()
        .unwrap()
        .to_owned();
    failed(f.call(&["run", "--jobs", "1"]), "explicit --retry");
    fs::write(gate, "fixed").unwrap();
    passed(f.call(&["run", "--jobs", "1", "--retry", "a"]));
    let state = f.state();
    let item = &state["tasks"]["a"];
    assert_eq!(item["history"].as_array().unwrap().len(), 1);
    assert_ne!(item["worker_path"], first);
    assert!(Path::new(&first).is_dir());
    assert_eq!(item["history"][0]["status"], "blocked");
    assert!(item["history"][0]["worker_command"].is_array());
}

#[test]
fn worker_check_failure_does_not_commit_or_unblock_dependents() {
    let mut f = Fixture::new();
    f.data["tickets"][0]["checks"] = json!([[worker_binary(), "check-fail"]]);
    f.add_ticket("b", &["a"], "ok", &["b.txt"]);
    failed(f.call(&["run", "--jobs", "2"]), "check failed (4)");
    let state = f.state();
    assert_eq!(state["tasks"]["a"]["status"], "blocked");
    assert_eq!(state["tasks"]["b"]["status"], "pending");
    assert_eq!(f.git(&["rev-parse", "HEAD"]), f.initial);
}

#[test]
fn integration_check_retry_never_reapplies_ticket() {
    let mut f = Fixture::new();
    let gate = f.root.path().join("gate");
    f.data["integration_checks"] = json!([[worker_binary(), "check-exists", gate]]);
    f.add_ticket("b", &["a"], "need-a", &["b.txt"]);
    failed(f.call(&["run", "--jobs", "1"]), "check failed");
    let state = f.state();
    assert_eq!(state["tasks"]["a"]["status"], "blocked");
    assert_eq!(state["tasks"]["b"]["status"], "pending");
    let merged = state["tasks"]["a"]["integration_commit"].clone();
    fs::write(gate, "fixed").unwrap();
    passed(f.call(&["run", "--jobs", "1", "--retry", "a"]));
    let state = f.state();
    assert_eq!(state["tasks"]["a"]["integration_commit"], merged);
    assert_eq!(state["tasks"]["a"]["history"].as_array().unwrap().len(), 1);
    assert_eq!(
        f.git(&["log", "--format=%s"])
            .matches("feat: implement a")
            .count(),
        1
    );
}

#[test]
fn graph_and_brief_fingerprints_and_explicit_head_adoption() {
    let mut f = Fixture::new();
    passed(f.call(&["run", "--jobs", "1"]));
    fs::write(f.work.join("a.md"), "different intent").unwrap();
    failed(f.call(&["status"]), "graph or briefs changed");
    fs::write(f.work.join("a.md"), "Deliver a. Test its behavior.\n").unwrap();
    f.data["tickets"][0]["deliverable"] = json!("new scope");
    failed(f.call(&["status"]), "graph or briefs changed");
    f.data["tickets"][0]["deliverable"] = json!("deliver a");
    fs::write(f.repo.join("repair.txt"), "repair").unwrap();
    f.git(&["add", "repair.txt"]);
    f.git(&["commit", "-m", "fix: repair"]);
    let head = f.git(&["rev-parse", "HEAD"]);
    failed(f.call(&["run", "--jobs", "1"]), "HEAD drifted");
    failed(
        f.call(&["run", "--jobs", "1", "--accept-head", &f.initial]),
        "HEAD drifted",
    );
    passed(f.call(&["run", "--jobs", "1", "--accept-head", &head]));
    assert_eq!(f.state()["adopted_heads"].as_array().unwrap().len(), 1);
    assert_eq!(f.state()["adopted_heads"][0]["status"], "verified");
}

#[test]
fn worker_timeout_preserves_work_and_blocks_ticket() {
    let mut f = Fixture::new();
    f.mode(0, "slow", &[]);
    failed(
        f.call(&["run", "--jobs", "1", "--worker-timeout", "0.2"]),
        "timed out",
    );
    let state = f.state();
    let item = &state["tasks"]["a"];
    assert_eq!(item["status"], "blocked");
    assert!(item["reason"].as_str().unwrap().contains("timed out"));
    assert!(Path::new(item["worker_path"].as_str().unwrap()).is_dir());
}

#[test]
fn locks_and_dirty_checkout_prevent_launch() {
    let f = Fixture::new();
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(f.repo.join(".agent/work/.dispatch.lock"))
        .unwrap();
    FileExt::try_lock_exclusive(&lock).unwrap();
    failed(f.call(&["run", "--jobs", "1"]), "already running");
    FileExt::unlock(&lock).unwrap();
    fs::write(f.repo.join("README.md"), "uncommitted").unwrap();
    failed(f.call(&["run", "--jobs", "1"]), "must be clean");
    assert!(!f.work.join("dispatch/state.json").exists());
}

#[test]
fn completed_merge_reconciled_after_state_write_interruption() {
    let f = Fixture::new();
    passed(f.call(&["run", "--jobs", "1"]));
    let mut state = f.state();
    let head = state["integration_head"].clone();
    state["tasks"]["a"]["status"] = json!("merging");
    state["tasks"]["a"]
        .as_object_mut()
        .unwrap()
        .remove("integration_commit");
    state["tasks"]["a"]
        .as_object_mut()
        .unwrap()
        .remove("integration_checks");
    state["integration_head"] = json!(f.initial);
    f.save_state(&state);
    passed(f.call(&["run", "--jobs", "1"]));
    assert_eq!(f.state()["tasks"]["a"]["status"], "integrated");
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head.as_str().unwrap());
    assert_eq!(
        f.git(&["log", "--format=%s"])
            .matches("feat: implement a")
            .count(),
        1
    );
}

#[test]
fn missing_cli_blocks_without_fallback() {
    let mut f = Fixture::new();
    f.data["tickets"][0]["worker_command"] = json!([f.root.path().join("missing-cli")]);
    failed(f.call(&["run", "--jobs", "1"]), "No such file");
    assert_eq!(f.state()["tasks"]["a"]["status"], "blocked");
    assert_eq!(f.git(&["rev-parse", "HEAD"]), f.initial);
}

#[test]
fn finished_worker_does_not_timeout_during_integration_checks() {
    let mut f = Fixture::new();
    f.add_ticket("b", &[], "ok", &["b.txt"]);
    f.data["integration_checks"] = json!([[worker_binary(), "check-sleep", "2500"]]);
    passed(f.call(&["run", "--jobs", "2", "--worker-timeout", "2"]));
    assert_eq!(f.state()["tasks"]["b"]["status"], "integrated");
}

#[test]
fn failed_worker_children_stopped_before_retry() {
    let mut f = Fixture::new();
    let pid = f.root.path().join("child.pid");
    let gate = f.root.path().join("gate");
    f.mode(0, "child-fail", &[&pid, &gate]);
    failed(f.call(&["run", "--jobs", "1"]), "worker failed (7)");
    stopped(&pid);
    fs::write(gate, "fixed").unwrap();
    passed(f.call(&["run", "--jobs", "1", "--retry", "a"]));
}

#[test]
fn unrelated_integration_edits_preserved_before_merge() {
    let mut f = Fixture::new();
    let target = f.repo.join("README.md");
    f.mode(0, "main-dirty", &[&target]);
    failed(f.call(&["run", "--jobs", "1"]), "preserve and reconcile");
    assert_eq!(fs::read_to_string(target).unwrap(), "unrelated edit");
    assert_eq!(f.git(&["rev-parse", "HEAD"]), f.initial);
    assert_eq!(f.state()["tasks"]["a"]["status"], "blocked");
}

#[test]
fn deadline_enforced_during_checks_before_new_launches() {
    let mut f = Fixture::new();
    let began = f.root.path().join("began");
    let stopped_at = f.root.path().join("stopped");
    f.add_ticket("b", &[], "ok", &["b.txt"]);
    f.data["tickets"][1]["worker_command"] = json!([
        "sh",
        "-c",
        "date +%s > \"$1\"; trap 'date +%s > \"$2\"; exit 8' TERM; while :; do sleep 0.1; done",
        "deadline",
        began,
        stopped_at
    ]);
    f.add_ticket("c", &["a"], "ok", &["c.txt"]);
    f.data["integration_checks"] = json!([[worker_binary(), "check-sleep", "4000"]]);
    failed(
        f.call(&["run", "--jobs", "2", "--worker-timeout", "2"]),
        "worker timed out: b",
    );
    let start: u64 = fs::read_to_string(began).unwrap().trim().parse().unwrap();
    let stop: u64 = fs::read_to_string(stopped_at)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(stop - start <= 3, "deadline elapsed: {}", stop - start);
    let state = f.state();
    assert_eq!(state["tasks"]["b"]["status"], "blocked");
    assert_eq!(state["tasks"]["c"]["status"], "pending");
}

#[test]
fn blocked_retry_refuses_live_previous_worker_group() {
    let mut f = Fixture::new();
    let gate = f.root.path().join("gate");
    f.mode(0, "fail", &[&gate]);
    failed(f.call(&["run", "--jobs", "1"]), "worker failed");
    let mut child = Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .unwrap();
    let mut state = f.state();
    state["tasks"]["a"]["pid"] = json!(child.id());
    f.save_state(&state);
    let result = f.call(&["run", "--jobs", "1", "--retry", "a"]);
    child.kill().unwrap();
    child.wait().unwrap();
    failed(result, "previous worker group is still live");
    assert_eq!(f.state()["tasks"]["a"]["history"], json!([]));
}

#[test]
fn check_timeout_and_check_descendants_are_stopped() {
    let mut f = Fixture::new();
    f.data["tickets"][0]["checks"] = json!([[worker_binary(), "check-sleep", "30000"]]);
    failed(
        f.call(&["run", "--jobs", "1", "--check-timeout", "0.2"]),
        "check timed out",
    );
    assert_eq!(f.git(&["rev-parse", "HEAD"]), f.initial);
    let mut f = Fixture::new();
    let pid = f.root.path().join("child.pid");
    f.data["tickets"][0]["checks"] = json!([[worker_binary(), "check-child", pid]]);
    failed(
        f.call(&["run", "--jobs", "1"]),
        "check left live child processes",
    );
    stopped(&pid);
}

#[test]
fn worker_must_leave_head_branch_and_nonempty_owned_edits() {
    for (mode, error) in [
        ("branch", "worker branch changed"),
        ("commit", "changed HEAD"),
        ("empty", "check failed"),
    ] {
        let mut f = Fixture::new();
        f.mode(0, mode, &[]);
        failed(f.call(&["run", "--jobs", "1"]), error);
        assert_eq!(f.git(&["rev-parse", "HEAD"]), f.initial);
    }

    let mut f = Fixture::new();
    f.mode(0, "empty", &[]);
    f.data["tickets"][0]["checks"] = json!([[worker_binary(), "check-exists", "README.md"]]);
    failed(f.call(&["run", "--jobs", "1"]), "produced no changes");
}

#[test]
fn foreground_cancellation_stops_worker_groups_and_retains_state() {
    let mut f = Fixture::new();
    f.mode(0, "slow", &[]);
    let mut child = f
        .command(&["run", "--jobs", "1"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let state = loop {
        if let Ok(bytes) = fs::read(f.work.join("dispatch/state.json"))
            && let Ok(state) = serde_json::from_slice::<Value>(&bytes)
            && state["tasks"]["a"]["pid"].is_u64()
        {
            break state;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    };
    kill(Pid::from_raw(child.id() as i32), Signal::SIGTERM).unwrap();
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }

    assert_eq!(f.state()["tasks"]["a"]["status"], "blocked");
    let pid_file = f.root.path().join("pid");
    fs::write(&pid_file, state["tasks"]["a"]["pid"].to_string()).unwrap();
    stopped(&pid_file);
}

#[test]
fn help_exits_successfully_without_opening_a_repository() {
    let result = Command::new(env!("CARGO_BIN_EXE_dispatch-tickets"))
        .arg("--help")
        .output()
        .unwrap();
    let result = passed(result);
    assert!(String::from_utf8_lossy(&result.stdout).contains("--graph"));
}
