//! Operating-system operations of the controller: the processes of a Gateway
//! session, private files, signals and the agent path. The Unix and Windows
//! modules offer the same functions.
//!
//! A session's processes: on Unix, process groups (the launcher's group,
//! which Gateway's JVMs and its restart child stay in, and the groups of
//! fresh agents); on Windows, one Job Object per session, named after the
//! session ID, which the launcher enters before it runs. A group ID (the
//! launcher's PID) identifies the session's processes on both.

#[cfg(unix)]
pub use unix::*;
#[cfg(windows)]
pub use windows::*;

/// A controller signal.
pub enum Signal {
    /// End the controller (SIGTERM; console close, logoff or shutdown).
    Terminate,
    /// End the controller (Ctrl-C).
    Interrupt,
    /// Read the settings again (SIGHUP; none on Windows).
    Reload,
    /// End the controller at once and leave Gateway running (Windows console
    /// close, logoff or shutdown: Windows ends the process within seconds,
    /// so a stop cannot finish). The next controller adopts Gateway.
    Leave,
}

#[cfg(unix)]
mod unix {
    use super::Signal;
    use anyhow::{Context, Result};
    use std::{
        collections::HashSet,
        fs::{self, OpenOptions},
        os::unix::{
            fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
            process::CommandExt,
        },
        path::{Path, PathBuf},
    };
    use tokio::signal::unix::{SignalKind, signal};

    /// The library name of the native agent.
    pub const AGENT_LIBRARY: &str = if cfg!(target_os = "macos") {
        "libibkit_gateway_agent.dylib"
    } else {
        "libibkit_gateway_agent.so"
    };

    /// Run a helper (the display, VNC) in its own process group, so that a
    /// signal to the controller's group does not end it first.
    pub fn isolate(command: &mut tokio::process::Command) {
        command.as_std_mut().process_group(0);
    }

    /// Run the launcher as the leader of a new process group, so that the
    /// session's processes can be found and stopped as one.
    pub fn isolate_launcher(command: &mut tokio::process::Command) {
        command.as_std_mut().process_group(0);
    }

    /// The process group of the launcher with `pid` that `isolate` made.
    pub fn session_spawned(pid: u32, _session: &str) -> Result<u32> {
        Ok(pid)
    }

    /// Whether process group `group` has a process (signal 0 to the group; a
    /// group of another user also exists).
    pub fn group_alive(group: u32, _session: &str) -> bool {
        rustix::process::Pid::from_raw(group as i32).is_some_and(|pid| {
            matches!(
                rustix::process::test_kill_process_group(pid),
                Ok(()) | Err(rustix::io::Errno::PERM)
            )
        })
    }

    /// Ask the session's processes to end (`force`: kill them).
    pub fn end_groups(groups: &HashSet<u32>, _session: &str, force: bool) {
        let signal = if force {
            rustix::process::Signal::KILL
        } else {
            rustix::process::Signal::TERM
        };
        for group in groups {
            if let Some(pid) = rustix::process::Pid::from_raw(*group as i32) {
                let _ = rustix::process::kill_process_group(pid, signal);
            }
        }
    }

    /// The process group of `pid` when it is not this controller's own group.
    pub fn foreign_group(pid: u32) -> Option<u32> {
        let pid = rustix::process::Pid::from_raw(pid as i32)?;
        let group = rustix::process::getpgid(Some(pid)).ok()?;
        let own = rustix::process::getpgid(None).ok()?;
        (group != own).then(|| group.as_raw_nonzero().get() as u32)
    }

    /// A saved group that a replacement controller may control. Never on
    /// Unix: a group ID from `status.json` can be reused (a new container).
    pub fn reopen_group(_saved: u32, _session: &str) -> Option<u32> {
        None
    }

    /// Whether process `pid` exists.
    pub fn process_alive(pid: u32) -> bool {
        rustix::process::Pid::from_raw(pid as i32).is_some_and(|pid| {
            matches!(
                rustix::process::test_kill_process(pid),
                Ok(()) | Err(rustix::io::Errno::PERM)
            )
        })
    }

    /// Ask process `pid` to end (`force`: kill it).
    pub fn end_process(pid: u32, force: bool) {
        if let Some(pid) = rustix::process::Pid::from_raw(pid as i32) {
            let _ = rustix::process::kill_process(
                pid,
                if force {
                    rustix::process::Signal::KILL
                } else {
                    rustix::process::Signal::TERM
                },
            );
        }
    }

    /// Create `path` as a directory that only this user can open.
    pub fn private_dir(path: &Path) -> std::io::Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
    }

    /// Open options for a file that only this user can read.
    pub fn private_file(options: &mut OpenOptions) -> &mut OpenOptions {
        options.mode(0o600)
    }

    /// A temporary directory that only this user can open, in `within`
    /// (default: the system temporary directory).
    pub fn private_tempdir(
        prefix: &str,
        within: Option<&Path>,
    ) -> std::io::Result<tempfile::TempDir> {
        let mut builder = tempfile::Builder::new();
        builder
            .prefix(prefix)
            .permissions(fs::Permissions::from_mode(0o700));
        match within {
            Some(directory) => builder.tempdir_in(directory),
            None => builder.tempdir(),
        }
    }

    /// Replace `path` with `file` (atomic rename).
    pub fn persist(file: tempfile::NamedTempFile, path: &Path) -> std::io::Result<()> {
        file.persist(path).map(drop).map_err(|error| error.error)
    }

    /// The user this controller runs as, for error messages.
    pub fn user() -> String {
        format!("uid {}", rustix::process::getuid().as_raw())
    }

    /// Some container runtimes set an open-file limit near 2^30. The JVM can
    /// then fail at startup ("unable to allocate file descriptor table").
    /// Gateway needs far fewer descriptors, so lower the limit the child
    /// inherits.
    pub fn cap_open_files() -> Result<()> {
        use rustix::process::{Resource, getrlimit, setrlimit};
        const CAP: u64 = 1 << 20;
        let mut limit = getrlimit(Resource::Nofile);
        if limit.current.is_none_or(|value| value > CAP)
            || limit.maximum.is_none_or(|value| value > CAP)
        {
            limit.current = Some(limit.current.map_or(CAP, |value| value.min(CAP)));
            limit.maximum = Some(limit.maximum.map_or(CAP, |value| value.min(CAP)));
            setrlimit(Resource::Nofile, limit).context("Cannot lower the open-file limit for Gateway. Set a container open-file limit of 1048576 or lower and start again.")?;
        }
        Ok(())
    }

    /// The agent path for JAVA_TOOL_OPTIONS. install4j parses java -version
    /// output during runtime selection, and a quoted JAVA_TOOL_OPTIONS entry
    /// can be mistaken for the version text, so a path with spaces or quotes
    /// gets an alias without them (kept alive by the returned directory).
    pub fn agent_path(agent: &Path) -> Result<(PathBuf, Option<tempfile::TempDir>)> {
        if !agent
            .to_string_lossy()
            .contains(|c: char| c.is_whitespace() || c == '"' || c == '\'')
        {
            return Ok((agent.to_path_buf(), None));
        }
        let alias = tempfile::Builder::new()
            .prefix("ibkit-agent-")
            .tempdir_in("/tmp")?;
        let target = alias.path().join(AGENT_LIBRARY);
        std::os::unix::fs::symlink(agent, &target)?;
        Ok((target, Some(alias)))
    }

    /// The controller's signals.
    pub struct Signals {
        terminate: tokio::signal::unix::Signal,
        interrupt: tokio::signal::unix::Signal,
        hangup: tokio::signal::unix::Signal,
    }

    impl Signals {
        pub fn new() -> Result<Self> {
            Ok(Self {
                terminate: signal(SignalKind::terminate())?,
                interrupt: signal(SignalKind::interrupt())?,
                hangup: signal(SignalKind::hangup())?,
            })
        }

        pub async fn next(&mut self) -> Signal {
            tokio::select! {
                _ = self.terminate.recv() => Signal::Terminate,
                _ = self.interrupt.recv() => Signal::Interrupt,
                _ = self.hangup.recv() => Signal::Reload,
            }
        }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows {
    use super::Signal;
    use anyhow::{Context, Result, bail};
    use std::{
        collections::HashSet,
        fs::{self, OpenOptions},
        path::{Path, PathBuf},
        time::Duration,
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, STILL_ACTIVE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
                JobObjectBasicAccountingInformation, OpenJobObjectW, QueryInformationJobObject,
                TerminateJobObject,
            },
            Threading::{
                CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED, DETACHED_PROCESS, GetExitCodeProcess,
                OpenProcess, OpenThread, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA,
                PROCESS_TERMINATE, ResumeThread, THREAD_SUSPEND_RESUME, TerminateProcess,
            },
        },
    };

    /// The library name of the native agent.
    pub const AGENT_LIBRARY: &str = "ibkit_gateway_agent.dll";

    /// Job Object access rights (winnt.h; windows-sys has them only in its
    /// large SystemServices module).
    const JOB_OBJECT_QUERY: u32 = 0x0004;
    const JOB_OBJECT_TERMINATE: u32 = 0x0008;

    /// A Win32 handle, closed on drop.
    struct Owned(HANDLE);

    impl Owned {
        fn new(handle: HANDLE) -> Option<Self> {
            (!handle.is_null() && handle as isize != -1).then_some(Self(handle))
        }
    }

    // SAFETY: a Win32 handle can be used and closed from any thread.
    unsafe impl Send for Owned {}

    /// The open job handle of each session this controller started. Windows
    /// drops a kernel object's name when its last handle closes, even while
    /// processes run in the job, so `open_job` finds a job only while a
    /// handle stays open.
    static JOBS: std::sync::Mutex<Vec<(String, Owned)>> = std::sync::Mutex::new(Vec::new());

    fn forget_job(session: &str) {
        JOBS.lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(name, _)| name != session);
    }

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: the handle is open and owned by this value.
            unsafe { CloseHandle(self.0) };
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    /// The Job Object name of a session, in the user's logon session.
    fn job_name(session: &str) -> Vec<u16> {
        wide(&format!("Local\\ibkit-{session}"))
    }

    fn open_job(session: &str, access: u32) -> Option<Owned> {
        // SAFETY: the name is a NUL-terminated UTF-16 string.
        Owned::new(unsafe { OpenJobObjectW(access, 0, job_name(session).as_ptr()) })
    }

    /// No helpers (a display or VNC) run on Windows.
    pub fn isolate(_command: &mut tokio::process::Command) {}

    /// Start the launcher suspended, detached from the controller's console
    /// (Ctrl-C must not reach Gateway before it logs out), in a new process
    /// group. `session_spawned` puts it in the session's job, then resumes it.
    pub fn isolate_launcher(command: &mut tokio::process::Command) {
        command.creation_flags(CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    }

    /// Put the suspended launcher `pid` in a new Job Object named after
    /// `session`, then let it run: every process it starts, also Gateway's
    /// restart child, is in the job. The job has no kill-on-close limit, so
    /// Gateway outlives the controller, and allows no breakaway.
    pub fn session_spawned(pid: u32, session: &str) -> Result<u32> {
        // SAFETY: plain Win32 calls with owned handles and valid pointers.
        unsafe {
            let job = Owned::new(CreateJobObjectW(
                std::ptr::null(),
                job_name(session).as_ptr(),
            ))
            .context("Cannot create the Job Object of the Gateway session")?;
            let process = Owned::new(OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid))
                .context("Cannot open the Gateway launcher process")?;
            if AssignProcessToJobObject(job.0, process.0) == 0 {
                TerminateProcess(process.0, 1);
                bail!(
                    "Cannot put the Gateway launcher in its Job Object: {}. Start the controller outside a job that forbids nested jobs.",
                    std::io::Error::last_os_error()
                );
            }
            resume_threads(pid)?;
            JOBS.lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((session.to_owned(), job));
        }
        Ok(pid)
    }

    /// Resume every thread of the suspended process `pid`.
    unsafe fn resume_threads(pid: u32) -> Result<()> {
        // SAFETY: the snapshot handle is owned; THREADENTRY32 has its size set.
        unsafe {
            let snapshot = Owned::new(CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0))
                .context("Cannot list the threads of the Gateway launcher")?;
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut more = Thread32First(snapshot.0, &mut entry) != 0;
            while more {
                if entry.th32OwnerProcessID == pid
                    && let Some(thread) =
                        Owned::new(OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID))
                {
                    ResumeThread(thread.0);
                }
                more = Thread32Next(snapshot.0, &mut entry) != 0;
            }
        }
        Ok(())
    }

    /// Whether the session's job still has a process.
    pub fn group_alive(_group: u32, session: &str) -> bool {
        let Some(job) = open_job(session, JOB_OBJECT_QUERY) else {
            return false;
        };
        // SAFETY: the buffer is a JOBOBJECT_BASIC_ACCOUNTING_INFORMATION.
        let alive = unsafe {
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
            QueryInformationJobObject(
                job.0,
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            ) != 0
                && info.ActiveProcesses > 0
        };
        if !alive {
            forget_job(session);
        }
        alive
    }

    /// End the session's processes. Windows has no polite signal for them:
    /// the agent already closed Gateway's window, so only `force` acts.
    pub fn end_groups(groups: &HashSet<u32>, session: &str, force: bool) {
        if force
            && !groups.is_empty()
            && let Some(job) = open_job(session, JOB_OBJECT_TERMINATE)
        {
            // SAFETY: the job handle is open with JOB_OBJECT_TERMINATE.
            unsafe { TerminateJobObject(job.0, 1) };
        }
    }

    /// Agents need no group lookup on Windows: the session job holds them.
    pub fn foreign_group(_pid: u32) -> Option<u32> {
        None
    }

    /// The saved group of a session whose job still runs: the job is named
    /// after the session, so a reused PID cannot match it.
    pub fn reopen_group(saved: u32, session: &str) -> Option<u32> {
        group_alive(saved, session).then_some(saved)
    }

    /// Whether process `pid` runs.
    pub fn process_alive(pid: u32) -> bool {
        // SAFETY: the process handle is owned; the exit code is an out value.
        unsafe {
            let Some(process) = Owned::new(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid))
            else {
                return false;
            };
            let mut code = 0u32;
            GetExitCodeProcess(process.0, &mut code) != 0 && code == STILL_ACTIVE as u32
        }
    }

    /// End process `pid` (`force` only; see `end_groups`).
    pub fn end_process(pid: u32, force: bool) {
        if !force {
            return;
        }
        // SAFETY: the process handle is owned and opened for termination.
        unsafe {
            if let Some(process) = Owned::new(OpenProcess(PROCESS_TERMINATE, 0, pid)) {
                TerminateProcess(process.0, 1);
            }
        }
    }

    /// Create `path`. Windows has no file modes: the directory's access list
    /// keeps it private, so keep state_dir under the user profile.
    pub fn private_dir(path: &Path) -> std::io::Result<()> {
        fs::create_dir_all(path)
    }

    /// Open options for a private file (see `private_dir`).
    pub fn private_file(options: &mut OpenOptions) -> &mut OpenOptions {
        options
    }

    /// A temporary directory in `within` (default: the user's temporary
    /// directory, private to the user).
    pub fn private_tempdir(
        prefix: &str,
        within: Option<&Path>,
    ) -> std::io::Result<tempfile::TempDir> {
        let mut builder = tempfile::Builder::new();
        builder.prefix(prefix);
        match within {
            Some(directory) => builder.tempdir_in(directory),
            None => builder.tempdir(),
        }
    }

    /// Replace `path` with `file`. A scanner or the indexer can hold `path`
    /// for a moment, so the rename is retried for up to a second.
    ///
    /// `std::fs::rename` uses POSIX rename semantics where Windows has them,
    /// so it replaces a file that a reader holds open with delete sharing (as
    /// Rust, and most runtimes, open files). tempfile's `persist` uses
    /// `MoveFileExW`, which fails with access denied in that case.
    pub fn persist(file: tempfile::NamedTempFile, path: &Path) -> std::io::Result<()> {
        let temporary = file.into_temp_path();
        let mut attempts = 0;
        loop {
            match fs::rename(&temporary, path) {
                Ok(()) => {
                    // The file now lives at `path`: do not delete it on drop.
                    let _ = temporary.keep();
                    return Ok(());
                }
                Err(error) if attempts >= 20 => return Err(error),
                Err(_) => {
                    attempts += 1;
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }

    /// The user this controller runs as, for error messages.
    pub fn user() -> String {
        std::env::var("USERNAME").unwrap_or_else(|_| "this user".into())
    }

    /// No open-file limit to lower on Windows.
    pub fn cap_open_files() -> Result<()> {
        Ok(())
    }

    /// The agent path for JAVA_TOOL_OPTIONS, which splits at spaces: a path
    /// with spaces gets a copy in a directory without them (a symbolic link
    /// needs a privilege on Windows).
    pub fn agent_path(agent: &Path) -> Result<(PathBuf, Option<tempfile::TempDir>)> {
        let plain = |path: &Path| {
            !path
                .to_string_lossy()
                .contains(|c: char| c.is_whitespace() || c == '"' || c == '\'')
        };
        if plain(agent) {
            return Ok((agent.to_path_buf(), None));
        }
        let base = [
            std::env::temp_dir(),
            std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_default(),
        ]
        .into_iter()
        .find(|base| !base.as_os_str().is_empty() && plain(base))
        .context("The agent path has spaces and no temporary directory without spaces exists. Copy ibkit_gateway_agent.dll to a path without spaces and set agent to it.")?;
        let copy = tempfile::Builder::new()
            .prefix("ibkit-agent-")
            .tempdir_in(base)?;
        let target = copy.path().join(AGENT_LIBRARY);
        fs::copy(agent, &target)
            .context("Cannot copy the native agent to a path without spaces")?;
        Ok((target, Some(copy)))
    }

    /// The controller's console events.
    pub struct Signals {
        interrupt: tokio::signal::windows::CtrlC,
        brk: tokio::signal::windows::CtrlBreak,
        close: tokio::signal::windows::CtrlClose,
        logoff: tokio::signal::windows::CtrlLogoff,
        shutdown: tokio::signal::windows::CtrlShutdown,
    }

    impl Signals {
        pub fn new() -> Result<Self> {
            use tokio::signal::windows::{
                ctrl_break, ctrl_c, ctrl_close, ctrl_logoff, ctrl_shutdown,
            };
            Ok(Self {
                interrupt: ctrl_c()?,
                brk: ctrl_break()?,
                close: ctrl_close()?,
                logoff: ctrl_logoff()?,
                shutdown: ctrl_shutdown()?,
            })
        }

        pub async fn next(&mut self) -> Signal {
            tokio::select! {
                _ = self.interrupt.recv() => Signal::Interrupt,
                _ = self.brk.recv() => Signal::Terminate,
                _ = self.close.recv() => Signal::Leave,
                _ = self.logoff.recv() => Signal::Leave,
                _ = self.shutdown.recv() => Signal::Leave,
            }
        }
    }
}
