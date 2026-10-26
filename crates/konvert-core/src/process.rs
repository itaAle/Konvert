//! Spawning of engine processes.
//!
//! Arguments are always passed as an argument vector (never through a shell), so
//! file names with spaces, quotes or Unicode characters cannot inject commands.
//! On Windows every child is created without a console window and attached to a
//! Job Object configured to kill it when Konvert exits, even after a crash. On
//! Linux the kernel sends the child SIGKILL when Konvert dies (`PR_SET_PDEATHSIG`).
//! macOS has no equivalent: there the child is killed on normal exit and on
//! cancellation (`kill_on_drop`), and a run left behind by a crash only writes into
//! the temporary run directory, which is removed at the next start.

use std::path::Path;
use std::process::Stdio;

use tokio::process::{Child, Command};

/// Creates a command for an engine executable with Konvert's process defaults.
pub(crate) fn engine_command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW: do not flash a console window for each conversion.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(target_os = "linux")]
    {
        let parent = std::process::id();
        // SAFETY: the closure runs in the forked child before exec and only makes
        // async-signal-safe calls (prctl, getppid, _exit).
        unsafe {
            cmd.pre_exec(move || {
                // The signal is tied to the spawning thread; engines are spawned from
                // the runtime's worker threads, which live as long as Konvert.
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                // Konvert already died between fork and prctl: do not run orphaned.
                if libc::getppid() as u32 != parent {
                    libc::_exit(1);
                }
                Ok(())
            });
        }
    }
    cmd
}

/// Spawns `cmd` and ties the child's lifetime to Konvert's.
pub(crate) fn spawn(cmd: &mut Command) -> std::io::Result<Child> {
    let child = cmd.spawn()?;
    #[cfg(windows)]
    kill_on_exit::attach(&child);
    Ok(child)
}

#[cfg(windows)]
mod kill_on_exit {
    //! A process-wide Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
    //! The handle is intentionally never closed: the OS closes it when Konvert
    //! terminates (normally or not), which kills every attached FFmpeg process.

    use std::sync::OnceLock;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    struct Job(HANDLE);
    // SAFETY: a Job Object handle can be used from any thread.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    static JOB: OnceLock<Option<Job>> = OnceLock::new();

    fn job() -> Option<HANDLE> {
        JOB.get_or_init(|| {
            // SAFETY: plain Win32 calls with valid arguments; the handle is checked.
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle.is_null() {
                    return None;
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    (&raw const info).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 {
                    CloseHandle(handle);
                    return None;
                }
                Some(Job(handle))
            }
        })
        .as_ref()
        .map(|job| job.0)
    }

    pub(super) fn attach(child: &tokio::process::Child) {
        if let (Some(job), Some(process)) = (job(), child.raw_handle()) {
            // SAFETY: both handles are valid for the duration of the call.
            // Failure only means the child is not auto-killed on crash.
            unsafe {
                AssignProcessToJobObject(job, process as HANDLE);
            }
        }
    }
}
