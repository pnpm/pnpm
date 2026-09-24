#[cfg(any(target_os = "macos", test))]
use pnpm_config::Config;
#[cfg(any(target_os = "macos", test))]
use pnpm_network::redact_and_sanitize;
use pnpm_reporter::Reporter;
#[cfg(target_os = "macos")]
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel};
#[cfg(any(target_os = "macos", test))]
use std::{
    collections::HashSet,
    io,
    path::PathBuf,
    process::{Command, Output},
    time::Duration,
};
#[cfg(any(target_os = "macos", all(test, unix)))]
use std::{
    io::Read,
    process::{Child, Stdio},
    thread::{self, JoinHandle},
    time::Instant,
};

#[cfg(any(target_os = "macos", test))]
const TMUTIL_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(any(target_os = "macos", all(test, unix)))]
const TMUTIL_POLL_INTERVAL: Duration = Duration::from_millis(50);
#[cfg(any(target_os = "macos", test))]
const TMUTIL_MAX_PATHS_PER_BATCH: usize = 64;

#[cfg(target_os = "macos")]
pub(super) struct TimeMachineExclusions(Vec<PathBuf>);

#[cfg(not(target_os = "macos"))]
pub(super) struct TimeMachineExclusions;

#[cfg(target_os = "macos")]
impl TimeMachineExclusions {
    pub(super) fn empty() -> Self {
        Self(Vec::new())
    }

    pub(super) fn capture(
        config: &Config,
        execution: super::InstallExecution,
        project_dirs: &[PathBuf],
    ) -> Self {
        if execution.lockfile_only || execution.dry_run {
            return Self::empty();
        }
        Self(new_directories_to_exclude(config, project_dirs))
    }

    pub(super) async fn apply<Sink: Reporter>(self) {
        let paths: Vec<PathBuf> = self.0
            .into_iter()
            .filter(|path| path.is_dir())
            .collect();
        if paths.is_empty() {
            return;
        }
        let deadline = Instant::now() + TMUTIL_TIMEOUT;
        for paths in paths.chunks(TMUTIL_MAX_PATHS_PER_BATCH) {
            let Some(timeout) = deadline.checked_duration_since(Instant::now()) else {
                Sink::emit(&LogEvent::Global(GlobalLog {
                    level: LogLevel::Warn,
                    message: tmutil_timeout_message(),
                }));
                break;
            };
            let paths = paths.to_vec();
            let result = tokio::task::spawn_blocking(move || run_tmutil(&paths, timeout)).await;
            let timed_out = matches!(&result, Ok(Ok(None)));
            let Some(message) = tmutil_failure_message(result) else {
                continue;
            };
            Sink::emit(&LogEvent::Global(GlobalLog { level: LogLevel::Warn, message }));
            if timed_out {
                break;
            }
        }
    }
}

#[cfg(any(target_os = "macos", test))]
fn tmutil_failure_message(
    result: Result<io::Result<Option<Output>>, tokio::task::JoinError>,
) -> Option<String> {
    Some(match result {
        Ok(Ok(Some(output))) if output.status.success() => return None,
        Ok(Ok(Some(output))) => {
            let diagnostic = tmutil_diagnostic(&output);
            format!(
                "Failed to exclude newly created directories from Time Machine (tmutil exited with {}){diagnostic}.",
                output.status,
            )
        }
        Ok(Ok(None)) => tmutil_timeout_message(),
        Ok(Err(error)) => {
            format!("Failed to run tmutil while excluding directories from Time Machine: {error}")
        }
        Err(error) => {
            format!("The tmutil task failed while excluding directories from Time Machine: {error}")
        }
    })
}

#[cfg(any(target_os = "macos", test))]
fn tmutil_timeout_message() -> String {
    format!(
        "Timed out after {} seconds while excluding directories from Time Machine.",
        TMUTIL_TIMEOUT.as_secs(),
    )
}

#[cfg(target_os = "macos")]
fn run_tmutil(paths: &[PathBuf], timeout: Duration) -> io::Result<Option<Output>> {
    run_command(tmutil_command(paths), timeout)
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn run_command(mut command: Command, timeout: Duration) -> io::Result<Option<Output>> {
    let child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    // Keep the guard armed until a completed `try_wait` has reaped the
    // process. Any early return or panic kills and reaps it instead.
    let mut guard = ChildGuard(Some(child));
    let child = guard.0.as_mut().expect("child just spawned");
    let stdout = drain_pipe(child.stdout.take().expect("piped stdout"));
    let stderr = drain_pipe(child.stderr.take().expect("piped stderr"));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= timeout {
            // Returning with the guard armed kills and reaps the child.
            // Closing its pipes also lets the detached readers finish.
            return Ok(None);
        }
        thread::sleep(TMUTIL_POLL_INTERVAL);
    };
    guard.disarm();
    Ok(Some(Output { status, stdout: join_pipe(stdout)?, stderr: join_pipe(stderr)? }))
}

#[cfg(any(target_os = "macos", all(test, unix)))]
struct ChildGuard(Option<Child>);

#[cfg(any(target_os = "macos", all(test, unix)))]
impl ChildGuard {
    fn disarm(&mut self) {
        self.0 = None;
    }
}

#[cfg(any(target_os = "macos", all(test, unix)))]
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn drain_pipe(mut pipe: impl Read + Send + 'static) -> JoinHandle<io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        pipe.read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn join_pipe(reader: JoinHandle<io::Result<Vec<u8>>>) -> io::Result<Vec<u8>> {
    reader.join().map_err(|_| io::Error::other("tmutil output reader panicked"))?
}

#[cfg(any(target_os = "macos", test))]
fn tmutil_diagnostic(output: &Output) -> String {
    let bytes = if output.stderr.is_empty() { &output.stdout } else { &output.stderr };
    let message = String::from_utf8_lossy(bytes);
    let message = message.trim();
    if message.is_empty() { String::new() } else { format!(": {}", redact_and_sanitize(message)) }
}

#[cfg(not(target_os = "macos"))]
impl TimeMachineExclusions {
    pub(super) fn empty() -> Self {
        Self
    }

    pub(super) async fn apply<Sink: Reporter>(self) {
        let _ = (self, std::marker::PhantomData::<Sink>);
    }
}

#[cfg(any(target_os = "macos", test))]
fn new_directories_to_exclude(config: &Config, project_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut paths = Vec::with_capacity(project_dirs.len() + 3);
    let mut seen = HashSet::with_capacity(project_dirs.len() + 3);
    if config.macos_backup.exclude_modules_dir {
        push_missing(&mut paths, &mut seen, config.modules_dir.clone());
        for project_dir in project_dirs {
            push_missing(&mut paths, &mut seen, project_dir.join(config.modules_dir_relative()));
        }
        let virtual_store_dir = pnpm_fs::lexical_normalize(config.effective_virtual_store_dir());
        let covered_by_new_modules_dir = paths
            .iter()
            .any(|modules_dir| {
                modules_dir == &virtual_store_dir
                    || pnpm_fs::is_subdir(modules_dir, &virtual_store_dir)
            });
        if !covered_by_new_modules_dir {
            push_missing(&mut paths, &mut seen, virtual_store_dir);
        }
    }
    if config.macos_backup.exclude_store_dir {
        push_missing(&mut paths, &mut seen, config.store_dir.root().to_path_buf());
    }
    paths
}

#[cfg(any(target_os = "macos", test))]
fn push_missing(paths: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: PathBuf) {
    if !path.exists() && seen.insert(path.clone()) {
        paths.push(path);
    }
}

#[cfg(any(target_os = "macos", test))]
fn tmutil_command(paths: &[PathBuf]) -> Command {
    let mut command = Command::new("/usr/bin/tmutil");
    command.arg("addexclusion").args(paths);
    command
}

#[cfg(test)]
mod tests;
