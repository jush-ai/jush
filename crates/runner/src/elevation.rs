//! Elevation on Windows: whether the runner runs with the administrator rights
//! of the user's own account, as a program started with Run as administrator
//! does, and the User Account Control prompt that asks for them.
//!
//! Windows runs the programs of an account in the Administrators group with
//! the account's limited rights until a prompt elevates one. The runner stays
//! elevated through the installation's scheduled task, which runs with the
//! highest privileges once an elevated process registered it
//! (`crate::autostart`): Windows then starts it elevated at each logon and
//! each start of the task, with no prompt.
//!
//! ```text
//!   runner (limited) ── ShellExecuteEx "runas" ──▶ UAC prompt
//!        │                                            │ Yes
//!        │            jush-start.exe ◀────────────────┘ (elevated)
//!        │             └▶ jush-runner elevate --home …: registers the
//!        │                task with the highest privileges, exits 0
//!        └─ hands over: starts the task, which waits for this one to end,
//!           and ends, leaving its jobs to the elevated runner
//! ```
//!
//! - A runner of the task whose account may elevate asks by itself once an
//!   installation, after it first connects, while its task does not run
//!   elevated; the device's page asks again whenever the user wants
//!   ([`Elevator`]). A prompt that showed is kept in the installation as
//!   [`ASKED`], whatever its answer: Windows keeps a prompt on the desktop
//!   after the runner that asked ended, so a runner that asked at each start
//!   would pile them up.
//! - The prompt waits for its answer, however long nobody gives it, apart from
//!   the connection: the runner serves meanwhile, and says whether a prompt
//!   shows, which the device's page shows. A request while one shows asks
//!   nothing more.
//! - A runner whose task runs elevated already, from a grant whose runner did
//!   not hand over, hands over without asking.
//! - A runner the `Run` value starts cannot stay elevated, since Windows starts
//!   no elevated program from it, and never asks.
//! - A runner started elevated, as from an administrator's terminal or SSH
//!   session, registers the task elevated at once, with no prompt
//!   (`crate::autostart`).
//! - An elevated process does not inherit the environment of the one that
//!   asked, so the elevated runner is told its installation's directory.

use jush_runner_protocol::wire::Elevation;

/// The file name of the program that starts the runner with no window
/// (`crates/runner/start/main.rs`).
#[cfg(windows)]
pub const STARTER: &str = "jush-start.exe";

/// The installation's file that says a runner asked by itself and its
/// prompt showed: its presence alone.
#[cfg(windows)]
pub const ASKED: &str = "elevation-asked";

/// Whether this process runs elevated; none off Windows.
#[cfg(not(windows))]
pub fn current() -> Option<Elevation> {
    None
}

/// Whether this process runs elevated; none off Windows.
#[cfg(windows)]
pub fn current() -> Option<Elevation> {
    windows::current()
}

#[cfg(windows)]
pub use windows::{Elevator, place_starter, run_elevated};

#[cfg(windows)]
mod windows {
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use tokio::sync::{oneshot, watch};
    use tokio_util::sync::CancellationToken;

    use jush_runner_protocol::wire::{Elevation, ElevationOutcome};
    use jush_shared_artifacts::{Mode, Permissions, Publication};
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, GetLastError, HANDLE};
    use windows_sys::Win32::Security::{
        CheckTokenMembership, CreateWellKnownSid, GetTokenInformation, SECURITY_MAX_SID_SIZE, TOKEN_ELEVATION_TYPE,
        TOKEN_QUERY, TokenElevationType, TokenElevationTypeFull, TokenElevationTypeLimited,
        WinBuiltinAdministratorsSid,
    };
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetExitCodeProcess, INFINITE, OpenProcessToken, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    use super::{ASKED, STARTER};
    use crate::autostart::Registration;

    /// The program the runner carries for its installation.
    static STARTER_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/jush-start.exe"));

    pub fn current() -> Option<Elevation> {
        // SAFETY: the token handle is this process's own, queried into a
        // value of the size given and closed before return.
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return None;
            }
            let mut kind: TOKEN_ELEVATION_TYPE = 0;
            let mut length = 0u32;
            let read = GetTokenInformation(
                token,
                TokenElevationType,
                (&raw mut kind).cast(),
                size_of::<TOKEN_ELEVATION_TYPE>() as u32,
                &mut length,
            );
            CloseHandle(token);
            if read == 0 {
                return None;
            }
            Some(match kind {
                kind if kind == TokenElevationTypeFull => Elevation::Elevated,
                kind if kind == TokenElevationTypeLimited => Elevation::Elevatable,
                // A token that is not split: User Account Control is off,
                // or the account is not an administrator.
                _ if administrator() => Elevation::Elevated,
                _ => Elevation::Unavailable,
            })
        }
    }

    /// Whether this process's token holds the Administrators group.
    fn administrator() -> bool {
        // SAFETY: the SID buffer is of the largest size a SID takes, and
        // its size is passed with it.
        unsafe {
            let mut sid = [0u8; SECURITY_MAX_SID_SIZE as usize];
            let mut size = SECURITY_MAX_SID_SIZE;
            if CreateWellKnownSid(WinBuiltinAdministratorsSid, std::ptr::null_mut(), sid.as_mut_ptr().cast(), &mut size) == 0
            {
                return false;
            }
            let mut member = 0;
            CheckTokenMembership(std::ptr::null_mut(), sid.as_mut_ptr().cast(), &mut member) != 0 && member != 0
        }
    }

    /// Writes the program the runner carries as the installation's
    /// `jush-start.exe`, unless it holds it already. A copy that runs, as
    /// the task's does, cannot be replaced but can be renamed, so it moves
    /// aside to `releases/`, where the next update removes it.
    pub async fn place_starter(directory: &Path) -> io::Result<()> {
        let path = directory.join(STARTER);
        match tokio::fs::read(&path).await {
            Ok(found) if found == STARTER_BYTES => return Ok(()),
            Ok(_) => {
                let releases = directory.join("releases");
                tokio::fs::create_dir_all(&releases).await?;
                let aside = releases.join(format!(".replaced-start-{}.exe", uuid::Uuid::new_v4().simple()));
                tokio::fs::rename(&path, &aside).await?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        jush_shared_artifacts::publish_bytes(
            &path,
            STARTER_BYTES,
            Publication {
                mode: Mode::Replace,
                permissions: Permissions::Executable,
                durable: true,
            },
        )
        .await
        .map_err(io::Error::other)
    }

    /// How long a prompt that could not show takes to fail: ShellExecuteEx
    /// fails at once when no prompt can show, and holds a prompt that shows
    /// until someone answers it, so a request still waiting after this long
    /// has its prompt on the desktop.
    const FAILS_WITHIN: Duration = Duration::from_secs(2);

    /// What came of one prompt.
    enum Prompted {
        Granted,
        Refused,
        /// It could not show, or the elevated runner could not register the
        /// task; the log says which.
        Failed,
    }

    /// Where a runner's task stands, for a request to elevate.
    enum Task {
        /// Windows starts the runner from its `Run` value, which starts
        /// nothing elevated.
        Missing,
        Limited,
        Elevated,
    }

    /// The runner's requests to run elevated, for a runner of the scheduled
    /// task whose account may elevate. The prompt's own task outlives a
    /// connection, so a drop neither waits for its answer nor loses it.
    pub struct Elevator {
        registration: Registration,
        /// Whether this process considered asking by itself already; the
        /// installation keeps [`ASKED`] across processes.
        asked: AtomicBool,
        /// Whether a prompt shows now, which the connection reports.
        showing: watch::Sender<bool>,
        /// Cancelled once a prompt granted elevation: the registration ends,
        /// leaving its jobs to the elevated runner its task starts.
        granted: CancellationToken,
    }

    impl Elevator {
        /// The elevator of the installation `registration` registers, for
        /// the runner its task started, whose account may elevate and which
        /// runs limited; `showing` and `granted` are the registration's.
        pub fn new(
            registration: &Registration,
            showing: watch::Sender<bool>,
            granted: CancellationToken,
        ) -> Option<Arc<Self>> {
            (registration.supervised() && current() == Some(Elevation::Elevatable)).then(|| {
                Arc::new(Self {
                    registration: registration.clone(),
                    asked: AtomicBool::new(false),
                    showing,
                    granted,
                })
            })
        }

        fn asked_path(&self) -> PathBuf {
            self.registration.directory().join(ASKED)
        }

        async fn task(&self) -> Task {
            if !self.registration.exists().await {
                Task::Missing
            } else if self.registration.elevated().await {
                Task::Elevated
            } else {
                Task::Limited
            }
        }

        /// Hands over to the task, which runs elevated already.
        fn hand_over(&self, cause: &'static str) {
            tracing::info!(event = "elevation.granted", cause, reason = "task_elevated", "the task runs elevated already; the runner hands over to it");
            self.granted.cancel();
        }

        /// Asks once an installation by itself, when the runner's task does
        /// not run elevated; hands over when it does.
        pub async fn ask_by_itself(self: &Arc<Self>) {
            const CAUSE: &str = "by_itself";
            if self.asked.swap(true, Ordering::SeqCst) {
                return;
            }
            match self.task().await {
                Task::Missing => return,
                Task::Elevated => return self.hand_over(CAUSE),
                Task::Limited => {}
            }
            if tokio::fs::try_exists(self.asked_path()).await.unwrap_or(false) {
                tracing::info!(event = "elevation.not_asked", cause = "asked_before", "the runner runs limited: a runner of this installation asked before");
                return;
            }
            if matches!(self.show(CAUSE).await, ElevationOutcome::Shown)
                && let Err(error) = tokio::fs::write(self.asked_path(), b"").await
            {
                tracing::warn!(event = "elevation.asked_unkept", %error, "that the runner asked could not be kept; the next start asks again");
            }
        }

        /// Shows the prompt, unless one shows already, and answers once it
        /// shows or could not; `cause` names who asked, for the log. A task
        /// that runs elevated already hands over instead.
        pub async fn request(self: &Arc<Self>, cause: &'static str) -> ElevationOutcome {
            if *self.showing.borrow() {
                return ElevationOutcome::Shown;
            }
            match self.task().await {
                Task::Missing => {
                    tracing::warn!(event = "elevation.unavailable", cause, reason = "no_scheduled_task", "no prompt asked: Windows starts this runner from its Run value, which starts nothing elevated");
                    ElevationOutcome::Unavailable
                }
                Task::Elevated => {
                    self.hand_over(cause);
                    ElevationOutcome::Shown
                }
                Task::Limited => self.show(cause).await,
            }
        }

        /// Shows the prompt on its own task, unless one shows already, and
        /// answers once it shows or could not.
        async fn show(self: &Arc<Self>, cause: &'static str) -> ElevationOutcome {
            // One prompt at a time: the request that marks it shows it.
            let claimed = self.showing.send_if_modified(|showing| !std::mem::replace(showing, true));
            if !claimed {
                return ElevationOutcome::Shown;
            }
            tracing::info!(event = "elevation.asking", cause, "the runner asks to run elevated");
            let (failed, failure) = oneshot::channel();
            let elevator = self.clone();
            tokio::spawn(async move {
                let prompted = elevator.prompt(cause).await;
                elevator.showing.send_replace(false);
                let _ = failed.send(matches!(prompted, Prompted::Failed));
            });
            match tokio::time::timeout(FAILS_WITHIN, failure).await {
                Ok(Ok(true)) => ElevationOutcome::Unavailable,
                _ => ElevationOutcome::Shown,
            }
        }

        /// Shows the prompt and waits for its answer: a grant registers the
        /// task with the highest privileges and hands over, a No is kept.
        async fn prompt(&self, cause: &'static str) -> Prompted {
            let directory = self.registration.directory().to_owned();
            let starter = directory.join(STARTER);
            let parameters = format!(
                "\"{}\" elevate --home \"{}\"",
                directory.join(format!("{}.exe", jush_runner_protocol::release::RUNNER)).display(),
                directory.display()
            );
            let prompted = tokio::task::spawn_blocking(move || run_elevated(&starter, &parameters, Some(&directory))).await;
            let prompted = match prompted {
                Ok(Ok(0)) => Prompted::Granted,
                Ok(Ok(code)) => {
                    tracing::error!(event = "elevation.failed", cause, reason = "registration_failed", code, "the elevated runner could not register the task with the highest privileges");
                    Prompted::Failed
                }
                Ok(Err(error)) if error.raw_os_error() == Some(ERROR_CANCELLED as i32) => Prompted::Refused,
                Ok(Err(error)) => {
                    tracing::warn!(event = "elevation.failed", cause, reason = "no_prompt", %error, "no prompt could ask");
                    Prompted::Failed
                }
                Err(error) => {
                    tracing::error!(event = "elevation.failed", cause, reason = "prompt_panicked", %error, "the prompt's thread failed");
                    Prompted::Failed
                }
            };
            match prompted {
                Prompted::Granted => {
                    tracing::info!(event = "elevation.granted", cause, "the prompt granted elevation; the runner hands over to its task");
                    self.granted.cancel();
                }
                Prompted::Refused => {
                    tracing::info!(event = "elevation.refused", cause, "the prompt granted nothing");
                }
                Prompted::Failed => {}
            }
            prompted
        }
    }

    /// Starts `program` with `parameters` elevated, in `directory` or this
    /// process's, which shows the prompt, and waits for it: its exit code,
    /// or the error that kept it from starting, `ERROR_CANCELLED` for a
    /// prompt that granted nothing. It blocks until someone answers.
    pub fn run_elevated(program: &Path, parameters: &str, directory: Option<&Path>) -> io::Result<u32> {
        let wide = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let verb = wide("runas");
        let file = wide(&program.to_string_lossy());
        let parameters = wide(parameters);
        let directory = directory.map(|directory| wide(&directory.to_string_lossy()));
        // SAFETY: every string is NUL-terminated UTF-16 that outlives the
        // call, the structure is zeroed but for what is set, with its size,
        // and the process handle it returns is waited on and closed.
        unsafe {
            // The shell asks for COM on the calling thread; a thread that
            // has it already keeps its own.
            let _ = CoInitializeEx(std::ptr::null(), (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32);
            let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
            info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
            info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC;
            info.lpVerb = verb.as_ptr();
            info.lpFile = file.as_ptr();
            info.lpParameters = parameters.as_ptr();
            info.lpDirectory = directory.as_ref().map_or(std::ptr::null(), |directory| directory.as_ptr());
            info.nShow = SW_HIDE;
            if ShellExecuteExW(&mut info) == 0 {
                return Err(io::Error::from_raw_os_error(GetLastError() as i32));
            }
            if info.hProcess.is_null() {
                return Err(io::Error::other("Windows started the elevated program without a handle to it"));
            }
            WaitForSingleObject(info.hProcess, INFINITE);
            let mut code = 1u32;
            let read = GetExitCodeProcess(info.hProcess, &mut code);
            CloseHandle(info.hProcess);
            if read == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(code)
        }
    }
}
