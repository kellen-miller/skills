use super::{
    Arc, AtomicBool, Child, Command, CommandExt, Duration, Instant, Mutex, MutexGuard, Ordering,
    Pid, Result, Signal, anyhow, bail, ensure, signal, thread,
};

pub(super) fn process_alive(pid: u32) -> Result<bool> {
    let output = Command::new("ps").args(["-axo", "pgid=,stat="]).output()?;
    ensure!(output.status.success(), "cannot inspect process groups");
    Ok(String::from_utf8_lossy(&output.stdout).lines().any(|row| {
        let mut fields = row.split_whitespace();
        fields.next().and_then(|v| v.parse::<u32>().ok()) == Some(pid)
            && fields.next().is_some_and(|s| !s.starts_with('Z'))
    }))
}

pub(super) fn stop_process(child: &mut Child) -> Result<()> {
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

pub(super) fn watch_worker(
    child: &Mutex<Child>,
    cancelled: &AtomicBool,
    expired: &AtomicBool,
    timeout: Duration,
) -> Result<()> {
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

pub(super) fn settle_children(child: &mut Child) -> Result<bool> {
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

pub(super) struct Worker {
    pub(super) child: Arc<Mutex<Child>>,
    pub(super) cancelled: Arc<AtomicBool>,
    pub(super) expired: Arc<AtomicBool>,
    pub(super) watch_error: Arc<Mutex<Option<String>>>,
    pub(super) timer: Option<thread::JoinHandle<()>>,
}

impl Worker {
    pub(super) fn spawn(mut cmd: Command, timeout: Duration) -> Result<Self> {
        let child = Arc::new(Mutex::new(cmd.process_group(0).spawn()?));
        let cancelled = Arc::new(AtomicBool::new(false));
        let expired = Arc::new(AtomicBool::new(false));
        let watch_error = Arc::new(Mutex::new(None));
        let (watch_child, watch_cancelled, watch_expired, watch_failure) = (
            child.clone(),
            cancelled.clone(),
            expired.clone(),
            watch_error.clone(),
        );
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

    pub(super) fn child_process(&self) -> Result<MutexGuard<'_, Child>> {
        self.child
            .lock()
            .map_err(|_| anyhow!("worker process lock poisoned"))
    }

    pub(super) fn check_watcher(&self) -> Result<()> {
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

    pub(super) fn cancel_timer(&mut self) -> Result<()> {
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
