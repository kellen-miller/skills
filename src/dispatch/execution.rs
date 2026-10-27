use super::*;

pub(super) struct Execution<'a> {
    pub(super) repo: &'a Path,
    pub(super) graph: &'a Graph,
    pub(super) jobs: usize,
    pub(super) worker_timeout: Duration,
    pub(super) check_timeout: Duration,
    pub(super) retry: &'a [String],
    pub(super) accept_head: Option<&'a str>,
}

pub(super) struct RunState {
    pub(super) state_path: PathBuf,
    pub(super) state: State,
    pub(super) active: BTreeMap<String, Worker>,
    pub(super) current: Option<String>,
}

impl RunState {
    pub(super) fn load(repo: &Path, graph: &Graph, directory: &Path) -> Result<Self> {
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
    pub(super) fn execute(&self, directory: &Path) -> Result<()> {
        let mut run = RunState::load(self.repo, self.graph, directory)?;
        let result = self.run(&mut run, directory);
        if let Err(error) = &result {
            self.preserve_failed_run(&mut run, error);
        }

        result
    }

    pub(super) fn run(&self, run: &mut RunState, directory: &Path) -> Result<()> {
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

    pub(super) fn reconcile_merges(&self, run: &mut RunState) -> Result<()> {
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

    pub(super) fn prepare_run(&self, run: &mut RunState) -> Result<()> {
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
                    .is_some_and(|reference| commit(self.repo, reference)
                        .is_ok_and(|accepted| accepted == head))
                    && ancestor(self.repo, &run.state.integration_head, &head)?,
                "integration HEAD drifted; inspect and explicitly --accept-head <commit>"
            );
            run.state.adopted_heads.push(
                json!({"previous":run.state.integration_head,"accepted":head,"status":"checking"}),
            );
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

    pub(super) fn verify_adopted_heads(&self, run: &mut RunState, directory: &Path) -> Result<()> {
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

    pub(super) fn integrate_validated(&self, run: &mut RunState) -> Result<()> {
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

    pub(super) fn execute_tickets(&self, run: &mut RunState, directory: &Path) -> Result<()> {
        while run
            .state
            .tasks
            .values()
            .any(|item| item["status"] != "integrated")
        {
            ensure!(
                !interruption().load(Ordering::SeqCst),
                "received interruption signal"
            );
            self.check_active_workers(run)?;
            self.start_ready_workers(run, directory)?;
            ensure!(
                !run.active.is_empty(),
                "no runnable tickets; inspect graph and state"
            );
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

    pub(super) fn check_active_workers(&self, run: &mut RunState) -> Result<()> {
        for (id, worker) in &run.active {
            run.current = Some(id.clone());
            worker.check_watcher()?;
            ensure!(
                !worker.expired.load(Ordering::SeqCst),
                "worker timed out: {id}"
            );
            let status = worker.child_process()?.try_wait()?;
            let Some(status) = status else {
                continue;
            };
            ensure!(
                !worker.expired.load(Ordering::SeqCst),
                "worker timed out: {id}"
            );
            ensure!(
                status.success(),
                "worker failed ({}); see {}",
                status.code().unwrap_or(-1),
                text(&task_state(&run.state, id)?["worker_log"])?
            );
        }

        Ok(())
    }

    pub(super) fn start_ready_workers(&self, run: &mut RunState, directory: &Path) -> Result<()> {
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

    pub(super) fn start_worker(
        &self,
        run: &mut RunState,
        id: &str,
        directory: &Path,
    ) -> Result<Worker> {
        let ticket = self
            .graph
            .tickets
            .get(id)
            .context("graph is missing a runnable ticket")?;
        let history = task_state(&run.state, id)?["history"]
            .as_array()
            .context("task history is invalid")?;
        let attempt = history.len() + 1;
        let hash = hex::encode(Sha256::digest(
            self.graph.path.as_os_str().as_encoded_bytes(),
        ));
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
        let branch = format!(
            "{}-{id}-{attempt}",
            self.graph.branch.trim_start_matches("refs/heads/")
        );
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
        println!(
            "{}",
            json!({"event":"started","task":id,"worktree":worker_path})
        );
        Ok(worker)
    }

    pub(super) fn completed_workers(&self, run: &mut RunState) -> Result<Vec<String>> {
        let mut completed = Vec::new();
        for (id, worker) in &run.active {
            run.current = Some(id.clone());
            worker.check_watcher()?;
            ensure!(
                !worker.expired.load(Ordering::SeqCst),
                "worker timed out: {id}"
            );
            if worker.child_process()?.try_wait()?.is_some() {
                completed.push(id.clone());
            }
        }

        Ok(completed)
    }

    pub(super) fn finish_worker(&self, run: &mut RunState, id: &str) -> Result<()> {
        let worker = run
            .active
            .get_mut(id)
            .context("completed worker is missing")?;
        worker.cancel_timer()?;
        let mut child = worker.child_process()?;
        let settled = settle_children(&mut child)?;
        ensure!(
            !worker.expired.load(Ordering::SeqCst),
            "worker timed out: {id}"
        );
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
        task_state_mut(&mut run.state, id)?["expected_integration_head"] =
            json!(run.state.integration_head);
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

    pub(super) fn preserve_failed_run(&self, run: &mut RunState, error: &anyhow::Error) {
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
                    && task_state(&run.state, id)
                        .is_ok_and(|item| matches!(merge_matches(self.repo, item), Ok(true)));
                let next_status = if completed_merge {
                    "merging"
                } else {
                    "blocked"
                };
                match task_state_mut(&mut run.state, id) {
                    Ok(item) => {
                        item["status"] = json!(next_status);
                        item["reason"] = json!(format!("{error:#}"));
                    }
                    Err(error) => {
                        cleanup_errors.push(format!("cannot block current ticket {id}: {error:#}"))
                    }
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
                Err(error) => {
                    cleanup_errors.push(format!("cannot block active ticket {id}: {error:#}"))
                }
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

pub(super) fn write_worker_packet(
    graph: &Graph,
    worker_path: &Path,
    id: &str,
    head: &str,
    ticket: &Ticket,
) -> Result<PathBuf> {
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

pub(super) fn worker_paths_overlap(
    graph: &Graph,
    id: &str,
    active: &BTreeMap<String, Worker>,
) -> bool {
    let ticket = &graph.tickets[id];
    active.keys().any(|other| {
        ticket.paths.iter().any(|path| {
            graph.tickets[other].paths.iter().any(|owned| {
                path == owned
                    || path.starts_with(&format!("{owned}/"))
                    || owned.starts_with(&format!("{path}/"))
            })
        })
    })
}
