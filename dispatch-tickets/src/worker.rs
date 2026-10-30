use super::{
    Arc, AtomicBool, Child, Command, Duration, Instant, Mutex, MutexGuard, Ordering, Result,
    anyhow, bail, ensure, thread,
};
#[cfg(unix)]
use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};
#[cfg(unix)]
use std::os::unix::process::CommandExt;

// Workers and checks run in their own process group (Unix) or Job Object
// (Windows) so the dispatcher can stop and audit the whole process tree.
#[cfg(unix)]
pub(super) fn spawn_isolated(command: &mut Command) -> Result<Child> {
    Ok(command.process_group(0).spawn()?)
}

#[cfg(unix)]
pub(super) fn process_alive(pid: u32) -> Result<bool> {
    let output = Command::new("ps").args(["-axo", "pgid=,stat="]).output()?;
    ensure!(output.status.success(), "cannot inspect process groups");
    Ok(String::from_utf8_lossy(&output.stdout).lines().any(|row| {
        let mut fields = row.split_whitespace();
        fields.next().and_then(|v| v.parse::<u32>().ok()) == Some(pid)
            && fields.next().is_some_and(|s| !s.starts_with('Z'))
    }))
}

#[cfg(unix)]
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

/// Keeps SIGINT/SIGTERM (Unix) or console control events (Windows) flowing
/// into the dispatcher's interruption flag until dropped.
#[cfg(unix)]
pub(super) struct InterruptGuard(signal_hook::SigId, signal_hook::SigId);

#[cfg(unix)]
pub(super) fn register_interrupts() -> Result<InterruptGuard> {
    Ok(InterruptGuard(
        signal_hook::flag::register(signal_hook::consts::SIGTERM, super::interruption().clone())?,
        signal_hook::flag::register(signal_hook::consts::SIGINT, super::interruption().clone())?,
    ))
}

#[cfg(unix)]
impl Drop for InterruptGuard {
    fn drop(&mut self) {
        signal_hook::low_level::unregister(self.0);
        signal_hook::low_level::unregister(self.1);
    }
}

// Windows tracks each worker tree with a Job Object keyed by the root pid.
// KILL_ON_JOB_CLOSE also reaps the tree if the dispatcher dies.
#[cfg(windows)]
static JOBS: Mutex<std::collections::BTreeMap<u32, usize>> =
    Mutex::new(std::collections::BTreeMap::new());

#[cfg(windows)]
pub(super) fn spawn_isolated(command: &mut Command) -> Result<Child> {
    use std::{
        ffi::c_void,
        io, mem,
        os::windows::{io::AsRawHandle, process::CommandExt},
    };
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject,
            },
            Threading::CREATE_NEW_PROCESS_GROUP,
        },
    };

    let mut child = command.creation_flags(CREATE_NEW_PROCESS_GROUP).spawn()?;
    // SAFETY: plain Win32 calls; `job` is owned here until stored or closed, and
    // `limits` outlives the call that reads it.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    ensure!(
        !job.is_null(),
        "cannot create worker job: {}",
        io::Error::last_os_error()
    );
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast::<c_void>(),
            mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) != 0
            && AssignProcessToJobObject(job, child.as_raw_handle()) != 0
    };
    if !configured {
        let error = io::Error::last_os_error();
        unsafe { CloseHandle(job) };
        child.kill()?;
        child.wait()?;
        bail!("cannot place worker in job: {error}");
    }

    JOBS.lock()
        .map_err(|_| anyhow!("worker job registry poisoned"))?
        .insert(child.id(), job as usize);
    Ok(child)
}

#[cfg(windows)]
pub(super) fn process_alive(pid: u32) -> Result<bool> {
    use std::{ffi::c_void, io, mem};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_ACCESS_DENIED, STILL_ACTIVE},
        System::{
            JobObjects::{
                JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JobObjectBasicAccountingInformation,
                QueryInformationJobObject,
            },
            Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        },
    };

    let mut jobs = JOBS
        .lock()
        .map_err(|_| anyhow!("worker job registry poisoned"))?;
    if let Some(&job) = jobs.get(&pid) {
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { mem::zeroed() };
        // SAFETY: `job` is a live handle owned by the registry; `info` is a
        // correctly sized out buffer.
        let queried = unsafe {
            QueryInformationJobObject(
                job as *mut c_void,
                JobObjectBasicAccountingInformation,
                (&raw mut info).cast::<c_void>(),
                mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            ) != 0
        };
        ensure!(
            queried,
            "cannot inspect worker job: {}",
            io::Error::last_os_error()
        );
        if info.ActiveProcesses > 0 {
            return Ok(true);
        }

        jobs.remove(&pid);
        unsafe { CloseHandle(job as *mut c_void) };
        return Ok(false);
    }

    // Pids from a previous dispatcher run have no job here; check the root only.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Ok(io::Error::last_os_error().raw_os_error() == Some(ERROR_ACCESS_DENIED as i32));
    }

    let mut code = 0;
    let queried = unsafe { GetExitCodeProcess(process, &mut code) != 0 };
    unsafe { CloseHandle(process) };
    Ok(queried && code == STILL_ACTIVE as u32)
}

#[cfg(windows)]
pub(super) fn stop_process(child: &mut Child) -> Result<()> {
    use std::ffi::c_void;
    use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::TerminateJobObject};

    let job = JOBS
        .lock()
        .map_err(|_| anyhow!("worker job registry poisoned"))?
        .remove(&child.id());
    if let Some(job) = job {
        // SAFETY: the handle was removed from the registry, so this is its last use.
        let terminated = unsafe { TerminateJobObject(job as *mut c_void, 1) != 0 };
        unsafe { CloseHandle(job as *mut c_void) };
        if !terminated {
            child.kill()?;
        }
    } else if process_alive(child.id())? {
        child.kill()?;
    }

    child.wait()?;
    Ok(())
}

/// Keeps SIGINT/SIGTERM (Unix) or console control events (Windows) flowing
/// into the dispatcher's interruption flag until dropped.
#[cfg(windows)]
pub(super) struct InterruptGuard;

#[cfg(windows)]
unsafe extern "system" fn on_console_event(_: u32) -> i32 {
    super::interruption().store(true, Ordering::SeqCst);
    1
}

#[cfg(windows)]
pub(super) fn register_interrupts() -> Result<InterruptGuard> {
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;

    // SAFETY: the handler is a static fn that only stores an atomic flag.
    ensure!(
        unsafe { SetConsoleCtrlHandler(Some(on_console_event), 1) } != 0,
        "cannot register console handler: {}",
        std::io::Error::last_os_error()
    );
    Ok(InterruptGuard)
}

#[cfg(windows)]
impl Drop for InterruptGuard {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;

        // SAFETY: removes the handler registered in `register_interrupts`.
        unsafe { SetConsoleCtrlHandler(Some(on_console_event), 0) };
    }
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
        let child = Arc::new(Mutex::new(spawn_isolated(&mut cmd)?));
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
