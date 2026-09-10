use journal::{current_generation, remove_state_file, validate_real_directory};
use miette::{IntoDiagnostic, WrapErr as _};
use pnpm_crypto_hash::create_hex_hash;
use pnpm_workspace_task_scheduler::{TaskGraph, TaskKey, TaskNode};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    ffi::{OsStr, OsString},
    fs,
    fs::{File, OpenOptions},
    io,
    io::Write as _,
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
        let tasks: Vec<TaskIdentity> = graph
            .iter()
            .map(|(key, node)| {
                let id = task_id(node, workspace_dir);
                keys_by_id.insert(id.clone(), key.clone());
                ids_by_key.insert(key.clone(), id.clone());
                task_identity(node, id, graph, workspace_dir, &script_commands)
            })
            .collect();
        let invocation = invocation_hash(command, params, settings, tasks);
        let state_dir = workspace_dir.join("node_modules").join(STATE_DIR);
        let latest_state_path = state_dir.join(LATEST_STATE_FILE);
        Self { state_dir, latest_state_path, invocation, keys_by_id, ids_by_key }
    }

    pub fn read_completed_tasks(&self) -> miette::Result<Option<HashSet<TaskKey>>> {
        let Some(latest_run) = self.resumable_latest_run()? else { return Ok(None) };
        let (run, finished) = match self.newest_state(&latest_run) {
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
        Ok(self.completed_from_journal(&contents, &run))
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

    fn journal_path(&self, run: &str) -> PathBuf {
        self.state_dir.join(format!("{}.{run}.jsonl", self.invocation))
    }

    fn published_path(&self, run: &str) -> PathBuf {
        self.state_dir.join(format!("{}.{run}{PUBLISHED_SUFFIX}", self.invocation))
    }

    fn finished_path(&self, run: &str) -> PathBuf {
        self.state_dir.join(format!("{}.{run}{FINISHED_SUFFIX}", self.invocation))
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
        if !remove_state_file(&self.published_path)? {
            return Ok(());
        }
        remove_state_file(&self.file_path)?;
        Ok(())
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

fn task_identity(
    node: &TaskNode,
    id: TaskId,
    graph: &TaskGraph,
    workspace_dir: &Path,
    script_commands: &impl Fn(&TaskNode, &str) -> Vec<String>,
) -> TaskIdentity {
    let mut scripts: Vec<ScriptIdentity> = node
        .scripts
        .iter()
        .map(|name| ScriptIdentity { name: name.clone(), commands: script_commands(node, name) })
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
}

/// The invocation's identity hash over its command line and the sorted
/// settings and tasks.
fn invocation_hash(
    command: &str,
    params: &[String],
    settings: &[String],
    mut tasks: Vec<TaskIdentity>,
) -> String {
    tasks.sort_by(|left, right| {
        left.project.cmp(&right.project).then_with(|| left.task.cmp(&right.task))
    });
    let mut settings = settings.to_vec();
    settings.sort();
    let identity =
        serde_json::to_string(&InvocationIdentity { command, params, settings: &settings, tasks })
            .expect("task invocation identity serializes");
    create_hex_hash(&identity)
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

fn run_id(generation: u64) -> String {
    let sequence = RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{generation:012x}-{}-{sequence}", std::process::id())
}

#[cfg(test)]
mod tests;

fn initial_journal_contents(header: &StateHeader, completed: &[&TaskId]) -> String {
    let mut contents = serde_json::to_string(header).expect("task state header serializes");
    contents.push('\n');
    for id in completed {
        let record = TaskRecord {
            run: header.run.clone(),
            project: id.project.clone(),
            task: id.task.clone(),
        };
        contents.push_str(&serde_json::to_string(&record).expect("task record serializes"));
        contents.push('\n');
    }
    contents
}

/// Remove an unpublished journal if it cannot be reopened for appending.
fn open_journal_for_append(file_path: &Path) -> Result<File, StateStorageError> {
    OpenOptions::new().append(true).open(file_path).map_err(|error| {
        let _ = fs::remove_file(file_path);
        StateStorageError::io(error, "opening", file_path)
    })
}

mod journal;
