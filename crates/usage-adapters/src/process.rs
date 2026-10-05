use chrono_tz::Tz;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    task::JoinHandle,
    time::Instant,
};
use usage_core::{CancellationToken, CoreError, ReportKind};

pub const COLLECTOR_VERSION: &str = "ccusage-20.0.26";
pub const NORMALIZATION_VERSION: &str = "claude-code-1";

#[derive(Debug, Clone, Copy)]
pub struct RunnerLimits {
    pub timeout: Duration,
    /// Combined stdout + stderr budget, not two independent unbounded buffers.
    pub output_bytes: usize,
}
impl Default for RunnerLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            output_bytes: 16 * 1024 * 1024,
        }
    }
}

/// Runs only the pinned native ccusage executable with fixed Claude report arguments.
/// No shell, inherited user configuration, authentication environment or raw diagnostic output.
#[derive(Clone)]
pub struct ProcessRunner {
    executable: PathBuf,
    limits: RunnerLimits,
}
impl ProcessRunner {
    pub fn new(executable: impl AsRef<Path>, limits: RunnerLimits) -> Result<Self, CoreError> {
        if limits.timeout.is_zero()
            || limits.output_bytes == 0
            || limits.output_bytes > 64 * 1024 * 1024
        {
            return Err(CoreError::InvalidQuery);
        }
        let executable = executable.as_ref().canonicalize().map_err(io_error)?;
        verify_executable(&executable)?;
        Ok(Self { executable, limits })
    }

    pub async fn run_report(
        &self,
        kind: ReportKind,
        root: &Path,
        timezone: &str,
        cancellation: CancellationToken,
    ) -> Result<Vec<u8>, CoreError> {
        timezone
            .parse::<Tz>()
            .map_err(|_| CoreError::InvalidQuery)?;
        cancellation.check()?;
        // Recheck before every spawn, including replacements of the packaged executable.
        let executable = self.executable.clone();
        tokio::task::spawn_blocking(move || verify_executable(&executable))
            .await
            .map_err(|_| CoreError::CollectionFailed)??;
        let root = root.canonicalize().map_err(io_error)?;
        if root
            .to_str()
            .is_none_or(|s| s.contains(',') || s.contains('\0'))
        {
            return Err(CoreError::InvalidQuery);
        }
        let sandbox = tempfile::tempdir().map_err(io_error)?;
        let config = sandbox.path().join("ccusage.json");
        std::fs::write(&config, b"{}").map_err(io_error)?;
        let mut command = Command::new(&self.executable);
        command
            .args([
                "claude",
                match kind {
                    ReportKind::Daily => "daily",
                    ReportKind::Session => "session",
                },
                "--json",
                "--offline",
                "--breakdown",
                "--mode",
                "calculate",
                "--order",
                "asc",
                "--timezone",
                timezone,
                "--config",
            ])
            .arg(&config)
            .current_dir(sandbox.path())
            .env_clear()
            .env("CLAUDE_CONFIG_DIR", &root)
            .env("HOME", sandbox.path())
            .env("USERPROFILE", sandbox.path())
            .env("XDG_CONFIG_HOME", sandbox.path())
            .env("XDG_CACHE_HOME", sandbox.path())
            .env("NO_COLOR", "1")
            .env("HTTP_PROXY", "http://127.0.0.1:9")
            .env("HTTPS_PROXY", "http://127.0.0.1:9")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            for name in ["SystemRoot", "WINDIR"] {
                if let Some(value) = std::env::var_os(name) {
                    command.env(name, value);
                }
            }
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.as_std_mut().process_group(0);
        }
        cancellation.check()?;
        let mut child = command.spawn().map_err(io_error)?;
        let tree = match ChildTree::attach(&child) {
            Ok(tree) => tree,
            Err(error) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(error);
            }
        };
        let stdout = child.stdout.take().ok_or(CoreError::CollectionFailed)?;
        let stderr = child.stderr.take().ok_or(CoreError::CollectionFailed)?;
        let budget = Arc::new(AtomicUsize::new(0));
        let mut output = ReaderTask(tokio::spawn(read_bounded(
            stdout,
            budget.clone(),
            self.limits.output_bytes,
            true,
        )));
        let mut errors = ReaderTask(tokio::spawn(read_bounded(
            stderr,
            budget.clone(),
            self.limits.output_bytes,
            false,
        )));
        let deadline = Instant::now() + self.limits.timeout;
        let status = loop {
            if let Err(error) = cancellation.check() {
                break Err(error);
            }
            if budget.load(Ordering::Acquire) > self.limits.output_bytes {
                break Err(CoreError::OutputLimitExceeded);
            }
            if Instant::now() >= deadline {
                break Err(CoreError::Timeout);
            }
            tokio::select! {
                result = child.wait() => break result.map_err(io_error),
                _ = tokio::time::sleep(Duration::from_millis(20)) => {}
            }
        };
        // Also terminates descendants which might otherwise retain pipe handles after parent exit.
        drop(tree);
        if status.is_err() {
            let _ = child.kill().await;
        }
        let _ = child.wait().await;
        let bytes = (&mut output.0)
            .await
            .map_err(|_| CoreError::CollectionFailed)?;
        let stderr_result = (&mut errors.0)
            .await
            .map_err(|_| CoreError::CollectionFailed)?;
        let status = status?;
        let bytes = bytes?;
        stderr_result?;
        cancellation.check()?;
        if !status.success() {
            return Err(CoreError::CollectionFailed);
        }
        Ok(bytes)
    }
}

struct ReaderTask(JoinHandle<Result<Vec<u8>, CoreError>>);
impl Drop for ReaderTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn read_bounded(
    mut reader: impl AsyncRead + Unpin,
    budget: Arc<AtomicUsize>,
    limit: usize,
    retain: bool,
) -> Result<Vec<u8>, CoreError> {
    let mut result = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let count = reader
            .read(&mut buffer)
            .await
            .map_err(|_| CoreError::CollectionFailed)?;
        if count == 0 {
            return Ok(result);
        }
        if budget
            .fetch_add(count, Ordering::AcqRel)
            .saturating_add(count)
            > limit
        {
            return Err(CoreError::OutputLimitExceeded);
        }
        if retain {
            result.extend_from_slice(&buffer[..count]);
        }
    }
}
pub(crate) fn io_error(error: std::io::Error) -> CoreError {
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => CoreError::PermissionDenied,
        std::io::ErrorKind::NotFound => CoreError::SourceNotDetected,
        _ => CoreError::CollectionFailed,
    }
}
fn verify_executable(path: &Path) -> Result<(), CoreError> {
    let metadata = std::fs::metadata(path).map_err(io_error)?;
    if !metadata.is_file() || metadata.len() > 8 * 1024 * 1024 {
        return Err(CoreError::SchemaUnsupported);
    }
    let bytes = std::fs::read(path).map_err(io_error)?;
    let expected = if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "ba5311e2f982c93a6b94dde5ca5488755f0f881b9836075076d58765d75fd2ce"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "6d816ab7e989d475f19b8178330435209b31b7714496f1fcbc9bdf6951082d03"
    } else {
        return Err(CoreError::SchemaUnsupported);
    };
    if format!("{:x}", Sha256::digest(bytes)) != expected {
        return Err(CoreError::SchemaUnsupported);
    }
    Ok(())
}

#[cfg(windows)]
struct ChildTree(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
unsafe impl Send for ChildTree {}
#[cfg(windows)]
impl ChildTree {
    fn attach(child: &tokio::process::Child) -> Result<Self, CoreError> {
        use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};
        // Job handles are owned by this guard and never inherited by the child.
        unsafe {
            let handle = child.raw_handle().ok_or(CoreError::CollectionFailed)?;
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(CoreError::CollectionFailed);
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            ) == 0
                || AssignProcessToJobObject(job, handle as _) == 0
            {
                CloseHandle(job);
                return Err(CoreError::CollectionFailed);
            }
            Ok(Self(job))
        }
    }
}
#[cfg(windows)]
impl Drop for ChildTree {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(unix)]
struct ChildTree(i32);
#[cfg(unix)]
impl ChildTree {
    fn attach(child: &tokio::process::Child) -> Result<Self, CoreError> {
        Ok(Self(child.id().ok_or(CoreError::CollectionFailed)? as i32))
    }
}
#[cfg(unix)]
impl Drop for ChildTree {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-self.0, libc::SIGKILL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn stdout_and_discarded_stderr_share_the_same_limit() {
        let budget = Arc::new(AtomicUsize::new(0));
        assert_eq!(
            read_bounded(b"abc".as_slice(), budget.clone(), 5, true)
                .await
                .unwrap(),
            b"abc"
        );
        assert_eq!(
            read_bounded(b"xyz".as_slice(), budget, 5, false)
                .await
                .unwrap_err(),
            CoreError::OutputLimitExceeded
        );
    }
    #[tokio::test]
    async fn stderr_is_drained_without_becoming_diagnostic_text() {
        assert!(read_bounded(
            b"PRIVATE_PATH_AND_AUTH".as_slice(),
            Arc::new(AtomicUsize::new(0)),
            100,
            false
        )
        .await
        .unwrap()
        .is_empty());
    }
}
