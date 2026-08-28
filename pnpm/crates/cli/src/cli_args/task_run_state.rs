use miette::{IntoDiagnostic, WrapErr as _};
use pnpm_crypto_hash::create_hex_hash;
use pnpm_workspace_task_scheduler::{TaskGraph, TaskKey, TaskNode};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const STATE_VERSION: u8 = 1;
const STATE_DIR: &str = ".pnpm-task-run-state-v1";
const LATEST_STATE_FILE: &str = "latest.json";
const PUBLISHED_SUFFIX: &str = ".published";
const FINISHED_SUFFIX: &str = ".finished";
const START_LOCK_DIR: &str = "start.lock";
const LOCK_WAIT: Duration = Duration::from_secs(2);
const LOCK_ABANDONED_AFTER: Duration = Duration::from_secs(30);
const RUN_GENERATION_LENGTH: usize = 12;
static RUN_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
struct TaskId {
    project: String,
    task: String,
}

#[derive(Serialize)]
struct ScriptIdentity {
    name: String,
    commands: Vec<String>,
}

#[derive(Serialize)]
struct TaskIdentity {
    project: String,
    task: String,
    scripts: Vec<ScriptIdentity>,
    requested: bool,
    dependencies: Vec<TaskId>,
}

#[derive(Serialize)]
struct InvocationIdentity<'a> {
    command: &'a str,
    params: &'a [String],
    settings: &'a [String],
    tasks: Vec<TaskIdentity>,
}

#[derive(Serialize, Deserialize)]
struct StateHeader {
    version: u8,
    invocation: String,
    run: String,
}

#[derive(Serialize, Deserialize)]
struct TaskRecord {
    run: String,
    project: String,
    task: String,
}

#[derive(Serialize, Deserialize)]
struct FinishRecord {
    run: String,
    finished: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum JournalRecord {
    Task(TaskRecord),
    Finish(FinishRecord),
}

pub struct TaskRunExecutionSettings<'a> {
    pub extra_bin_paths: &'a [PathBuf],
    pub extra_env: &'a HashMap<String, String>,
    pub modules_dir: &'a Path,
    pub node_experimental_package_map: bool,
    pub node_options: Option<&'a str>,
    pub user_agent: &'a str,
}

pub fn task_run_execution_settings(opts: &TaskRunExecutionSettings<'_>) -> Vec<String> {
    let extra_bin_paths: Vec<String> =
        opts.extra_bin_paths.iter().map(|path| path.to_string_lossy().into_owned()).collect();
    let mut extra_env: Vec<(&String, &String)> = opts.extra_env.iter().collect();
    extra_env.sort_by_key(|(key, _)| *key);
    vec![
        format!(
            "extra-bin-paths={}",
            serde_json::to_string(&extra_bin_paths).expect("extra bin paths serialize")
        ),
        format!(
            "extra-env={}",
            serde_json::to_string(&extra_env).expect("extra environment serializes")
        ),
        format!("modules-dir={}", opts.modules_dir.to_string_lossy()),
        format!("node-experimental-package-map={}", opts.node_experimental_package_map),
        format!("node-options={}", opts.node_options.unwrap_or_default()),
        format!("user-agent={}", opts.user_agent),
    ]
}

pub struct TaskRunStateContext {
    state_dir: PathBuf,
    latest_state_path: PathBuf,
    invocation: String,
    keys_by_id: HashMap<TaskId, TaskKey>,
    ids_by_key: HashMap<TaskKey, TaskId>,
}

impl TaskRunStateContext {
    pub fn new(
        command: &str,
        params: &[String],
        settings: &[String],
        graph: &TaskGraph,
        workspace_dir: &Path,
        script_commands: impl Fn(&TaskNode, &str) -> Vec<String>,
    ) -> Self {
        let mut keys_by_id = HashMap::with_capacity(graph.len());
        let mut ids_by_key = HashMap::with_capacity(graph.len());
        let mut tasks: Vec<TaskIdentity> = graph
            .iter()
            .map(|(key, node)| {
                let id = task_id(node, workspace_dir);
                keys_by_id.insert(id.clone(), key.clone());
                ids_by_key.insert(key.clone(), id.clone());
                let mut scripts: Vec<ScriptIdentity> = node
                    .scripts
                    .iter()
                    .map(|name| ScriptIdentity {
                        name: name.clone(),
                        commands: script_commands(node, name),
                    })
                    .collect();
                scripts.sort_by(|left, right| left.name.cmp(&right.name));
                let mut dependencies: Vec<TaskId> = node
                    .dependencies
                    .iter()
                    .map(|dependency| task_id(&graph[dependency], workspace_dir))
                    .collect();
                dependencies.sort();
                TaskIdentity {
                    project: id.project,
                    task: id.task,
                    scripts,
                    requested: node.requested,
                    dependencies,
                }
            })
            .collect();
        tasks.sort_by(|left, right| {
            left.project.cmp(&right.project).then_with(|| left.task.cmp(&right.task))
        });
        let mut settings = settings.to_vec();
        settings.sort();
        let identity = serde_json::to_string(&InvocationIdentity {
            command,
            params,
            settings: &settings,
            tasks,
        })
        .expect("task invocation identity serializes");
        let invocation = create_hex_hash(&identity);
        let state_dir = workspace_dir.join("node_modules").join(STATE_DIR);
        let latest_state_path = state_dir.join(LATEST_STATE_FILE);
        Self { state_dir, latest_state_path, invocation, keys_by_id, ids_by_key }
    }

    pub fn read_completed_tasks(&self) -> miette::Result<Option<HashSet<TaskKey>>> {
        let state_directory_exists = match self.validate_state_directory(false) {
            Ok(exists) => exists,
            Err(error) if error.is_unavailable() => return Ok(None),
            Err(error) => return Err(error.into_report()),
        };
        if !state_directory_exists {
            return Ok(None);
        }
        let latest = match fs::read_to_string(&self.latest_state_path) {
            Ok(contents) => match serde_json::from_str::<StateHeader>(&contents) {
                Ok(header) => header,
                Err(_) => return Ok(None),
            },
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    || is_state_unavailable_error(&error) =>
            {
                return Ok(None);
            }
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("reading {}", self.latest_state_path.display()));
            }
        };
        if latest.version != STATE_VERSION
            || latest.invocation != self.invocation
            || !is_run_id(&latest.run)
        {
            return Ok(None);
        }
        let (run, finished) = match self.newest_state(&latest.run) {
            Ok(state) => state,
            Err(error) if error.is_unavailable() => return Ok(None),
            Err(error) => return Err(error.into_report()),
        };
        if finished {
            return Ok(None);
        }
        let file_path = self.journal_path(&run);
        let contents = match fs::read(&file_path) {
            Ok(contents) => contents,
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    || is_state_unavailable_error(&error) =>
            {
                return Ok(None);
            }
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("reading {}", file_path.display()));
            }
        };
        // A record is committed by its newline; a process killed during
        // append can leave only the final record torn.
        let Some(last_newline) = contents.iter().rposition(|byte| *byte == b'\n') else {
            return Ok(None);
        };
        let Ok(complete) = std::str::from_utf8(&contents[..last_newline]) else {
            return Ok(None);
        };
        let mut lines = complete.lines();
        let Some(header) = lines.next() else { return Ok(None) };
        let Ok(header) = serde_json::from_str::<StateHeader>(header) else { return Ok(None) };
        if header.version != STATE_VERSION
            || header.invocation != self.invocation
            || header.run != run
        {
            return Ok(None);
        }
        let mut completed = HashSet::new();
        for line in lines {
            let Ok(record) = serde_json::from_str::<JournalRecord>(line) else { return Ok(None) };
            match record {
                JournalRecord::Task(record) if record.run == header.run => {
                    let id = TaskId { project: record.project, task: record.task };
                    let Some(key) = self.keys_by_id.get(&id) else { return Ok(None) };
                    completed.insert(key.clone());
                }
                JournalRecord::Finish(record) if record.run == header.run && record.finished => {
                    return Ok(None);
                }
                _ => {}
            }
        }
        Ok(Some(completed))
    }

    pub fn start(&self, completed_tasks: &HashSet<TaskKey>) -> miette::Result<TaskRunState> {
        let mut completed: Vec<&TaskId> =
            completed_tasks.iter().map(|key| &self.ids_by_key[key]).collect();
        completed.sort();
        let (file_path, run, file) = match self.start_file(&completed) {
            Ok(state) => state,
            Err(error) if error.is_unavailable() => {
                let run = run_id(current_generation());
                (self.journal_path(&run), run, None)
            }
            Err(error) => return Err(error.into_report()),
        };
        Ok(TaskRunState {
            file_path,
            published_path: self.published_path(&run),
            finished_path: self.finished_path(&run),
            writer: Mutex::new(TaskRunStateWriter {
                file,
                run,
                completed: completed_tasks.clone(),
            }),
        })
    }

    fn start_file(
        &self,
        completed: &[&TaskId],
    ) -> Result<(PathBuf, String, Option<File>), StateStorageError> {
        self.validate_state_directory(true)?;
        let lock_path = self.state_dir.join(START_LOCK_DIR);
        let Some(lock) =
            pnpm_fs::DirLock::acquire(lock_path.clone(), LOCK_WAIT, LOCK_ABANDONED_AFTER)
                .map_err(|error| StateStorageError::io(error, "locking", &lock_path))?
        else {
            let run = run_id(current_generation());
            return Ok((self.journal_path(&run), run, None));
        };
        let run = self.next_run_id()?;
        let header = StateHeader {
            version: STATE_VERSION,
            invocation: self.invocation.clone(),
            run: run.clone(),
        };
        let mut contents = serde_json::to_string(&header).expect("task state header serializes");
        contents.push('\n');
        for id in completed {
            let record =
                TaskRecord { run: run.clone(), project: id.project.clone(), task: id.task.clone() };
            contents.push_str(&serde_json::to_string(&record).expect("task record serializes"));
            contents.push('\n');
        }
        let file_path = self.journal_path(&run);
        pnpm_fs::write_atomic(&file_path, contents.as_bytes())
            .map_err(|error| StateStorageError::io(error, "writing", &file_path))?;
        let file = match OpenOptions::new().append(true).open(&file_path) {
            Ok(file) => file,
            Err(error) => {
                let _ = fs::remove_file(&file_path);
                return Err(StateStorageError::io(error, "opening", &file_path));
            }
        };
        match lock.is_owner() {
            Ok(true) => {}
            Ok(false) => {
                drop(file);
                let _ = fs::remove_file(&file_path);
                return Ok((file_path, run, None));
            }
            Err(error) => {
                drop(file);
                let _ = fs::remove_file(&file_path);
                return Err(StateStorageError::io(error, "checking", &lock_path));
            }
        }
        let latest_write = pnpm_fs::write_atomic(
            &self.latest_state_path,
            serde_json::to_string(&header).expect("latest task state serializes").as_bytes(),
        );
        if let Err(error) = latest_write {
            drop(file);
            let _ = fs::remove_file(&file_path);
            return Err(StateStorageError::io(error, "writing", &self.latest_state_path));
        }
        let published_path = self.published_path(&run);
        if let Err(error) = pnpm_fs::write_atomic(&published_path, &[]) {
            drop(file);
            let _ = fs::remove_file(&file_path);
            let _ = fs::remove_file(&published_path);
            return Err(StateStorageError::io(error, "writing", &published_path));
        }
        self.cleanup_older_finished_state(&run);
        Ok((file_path, run, Some(file)))
    }

    fn journal_path(&self, run: &str) -> PathBuf {
        self.state_dir.join(format!("{}.{run}.jsonl", self.invocation))
    }

    fn published_path(&self, run: &str) -> PathBuf {
        self.state_dir.join(format!("{}.{run}{PUBLISHED_SUFFIX}", self.invocation))
    }

    fn finished_path(&self, run: &str) -> PathBuf {
        self.state_dir.join(format!("{}.{run}{FINISHED_SUFFIX}", self.invocation))
    }

    fn newest_state(&self, latest_run: &str) -> Result<(String, bool), StateStorageError> {
        let mut newest_run = latest_run.to_string();
        let prefix = format!("{}.", self.invocation);
        let entries = fs::read_dir(&self.state_dir)
            .map_err(|error| StateStorageError::io(error, "reading", &self.state_dir))?;
        let mut names = HashSet::new();
        for entry in entries {
            let entry =
                entry.map_err(|error| StateStorageError::io(error, "reading", &self.state_dir))?;
            names.insert(entry.file_name());
        }
        let mut finished =
            names.contains(OsStr::new(&format!("{prefix}{latest_run}{FINISHED_SUFFIX}")));
        for name in &names {
            let Some(name) = name.to_str() else { continue };
            let Some(name) = name.strip_prefix(&prefix) else { continue };
            let (run, candidate_finished) = if let Some(run) = name.strip_suffix(FINISHED_SUFFIX) {
                (run, true)
            } else if let Some(run) = name.strip_suffix(".jsonl") {
                let published_name = format!("{prefix}{run}{PUBLISHED_SUFFIX}");
                if !names.contains(OsStr::new(&published_name)) {
                    continue;
                }
                (run, false)
            } else {
                continue;
            };
            if !is_run_id(run) {
                continue;
            }
            if run_generation(run) > run_generation(&newest_run) {
                newest_run = run.to_string();
                finished = candidate_finished;
            } else if run == newest_run && candidate_finished {
                finished = true;
            }
        }
        Ok((newest_run, finished))
    }

    fn next_run_id(&self) -> Result<String, StateStorageError> {
        let mut newest_run = run_id(current_generation());
        match fs::read_to_string(&self.latest_state_path) {
            Ok(contents) => {
                if let Ok(latest) = serde_json::from_str::<StateHeader>(&contents)
                    && latest.invocation == self.invocation
                    && is_run_id(&latest.run)
                    && run_generation(&latest.run) > run_generation(&newest_run)
                {
                    newest_run = latest.run;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(StateStorageError::io(error, "reading", &self.latest_state_path));
            }
        }
        newest_run = self.newest_state(&newest_run)?.0;
        let generation = u64::from_str_radix(run_generation(&newest_run), 16)
            .expect("validated run generation")
            .saturating_add(1);
        Ok(run_id(generation))
    }

    fn cleanup_older_finished_state(&self, run: &str) {
        let Ok(entries) = fs::read_dir(&self.state_dir) else { return };
        let prefix = format!("{}.", self.invocation);
        let generation = run_generation(run);
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(name) = name.strip_prefix(&prefix) else { continue };
            let Some(older_run) = name.strip_suffix(FINISHED_SUFFIX) else { continue };
            if !is_run_id(older_run) || run_generation(older_run) >= generation {
                continue;
            }
            let _ = fs::remove_file(entry.path());
        }
    }

    fn validate_state_directory(&self, create: bool) -> Result<bool, StateStorageError> {
        let node_modules_dir = self.state_dir.parent().expect("task state directory has a parent");
        if !validate_real_directory(node_modules_dir, create)? {
            return Ok(false);
        }
        validate_real_directory(&self.state_dir, create)
    }
}

pub struct TaskRunState {
    file_path: PathBuf,
    published_path: PathBuf,
    finished_path: PathBuf,
    writer: Mutex<TaskRunStateWriter>,
}

struct TaskRunStateWriter {
    file: Option<File>,
    run: String,
    completed: HashSet<TaskKey>,
}

impl TaskRunState {
    pub fn record_passed(
        &self,
        key: &TaskKey,
        node: &TaskNode,
        workspace_dir: &Path,
    ) -> miette::Result<()> {
        let mut writer = self.writer.lock().expect("task state lock is not poisoned");
        if writer.file.is_none() {
            return Ok(());
        }
        if !writer.completed.insert(key.clone()) {
            return Ok(());
        }
        let id = task_id(node, workspace_dir);
        let record = TaskRecord { run: writer.run.clone(), project: id.project, task: id.task };
        let line = serde_json::to_string(&record).expect("task record serializes");
        let result = writeln!(
            writer.file.as_mut().expect("unfinished task state has an open file"),
            "{line}",
        );
        if let Err(error) = result {
            if is_state_unavailable_error(&error) {
                writer.file.take();
                drop(writer);
                let _ = fs::remove_file(&self.file_path);
                let _ = fs::remove_file(&self.published_path);
                return Ok(());
            }
            writer.completed.remove(key);
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("writing {}", self.file_path.display()));
        }
        Ok(())
    }

    pub fn finish(&self) -> miette::Result<()> {
        let mut writer = self.writer.lock().expect("task state lock is not poisoned");
        let Some(mut file) = writer.file.take() else {
            return Ok(());
        };
        let finish_record = FinishRecord { run: writer.run.clone(), finished: true };
        let line = serde_json::to_string(&finish_record).expect("finish record serializes");
        if let Err(error) = writeln!(file, "{line}")
            && !is_state_unavailable_error(&error)
        {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("writing {}", self.file_path.display()));
        }
        drop(file);
        drop(writer);
        if let Err(error) = pnpm_fs::write_atomic(&self.finished_path, &[]) {
            if is_state_unavailable_error(&error) {
                return Ok(());
            }
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("writing {}", self.finished_path.display()));
        }
        match fs::remove_file(&self.published_path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) if is_state_unavailable_error(&error) => return Ok(()),
            Err(error) => Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("removing {}", self.published_path.display()))?,
        }
        match fs::remove_file(&self.file_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) if is_state_unavailable_error(&error) => Ok(()),
            Err(error) => Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("removing {}", self.file_path.display())),
        }
    }
}

fn task_id(node: &TaskNode, workspace_dir: &Path) -> TaskId {
    let relative = pnpm_fs::relative_path(workspace_dir, &node.project);
    let project = if relative.as_os_str().is_empty() {
        ".".to_string()
    } else {
        relative.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/")
    };
    TaskId { project, task: node.task_name.clone() }
}

fn is_run_id(run: &str) -> bool {
    run.len() > RUN_GENERATION_LENGTH + 1
        && run.len() <= 128
        && run.as_bytes()[RUN_GENERATION_LENGTH] == b'-'
        && run[..RUN_GENERATION_LENGTH]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && run[RUN_GENERATION_LENGTH + 1..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte) || byte == b'-')
}

fn run_generation(run: &str) -> &str {
    &run[..RUN_GENERATION_LENGTH]
}

fn validate_real_directory(path: &Path, create: bool) -> Result<bool, StateStorageError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound && !create => return Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match fs::create_dir(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(StateStorageError::io(error, "creating", path));
                }
            }
            fs::symlink_metadata(path)
                .map_err(|error| StateStorageError::io(error, "inspecting", path))?
        }
        Err(error) => {
            return Err(StateStorageError::io(error, "inspecting", path));
        }
    };
    if metadata.file_type().is_symlink()
        || pnpm_fs::read_symlink_dir(path).is_ok()
        || !metadata.is_dir()
    {
        return Err(StateStorageError::UnsafePath(path.to_path_buf()));
    }
    Ok(true)
}

enum StateStorageError {
    Io { error: io::Error, operation: &'static str, path: PathBuf },
    UnsafePath(PathBuf),
}

impl StateStorageError {
    fn io(error: io::Error, operation: &'static str, path: &Path) -> Self {
        Self::Io { error, operation, path: path.to_path_buf() }
    }

    fn is_unavailable(&self) -> bool {
        matches!(self, Self::Io { error, .. } if is_state_unavailable_error(error))
    }

    fn into_report(self) -> miette::Report {
        match self {
            Self::Io { error, operation, path } => {
                let result: miette::Result<()> = Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("{operation} {}", path.display()));
                result.expect_err("task state storage error cannot succeed")
            }
            Self::UnsafePath(path) => {
                let path = path.display();
                miette::miette!(
                    code = "ERR_PNPM_UNSAFE_TASK_RUN_STATE_PATH",
                    "Refusing to use task run state directory at {} because it is a symbolic link or not a directory",
                    path,
                )
            }
        }
    }
}

fn is_state_unavailable_error(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem)
}

fn current_generation() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
}

fn run_id(generation: u64) -> String {
    let sequence = RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{generation:012x}-{}-{sequence}", std::process::id())
}

#[cfg(test)]
mod tests;
