use miette::{IntoDiagnostic, WrapErr as _};
use pnpm_crypto_hash::create_hex_hash;
use pnpm_workspace_task_scheduler::{TaskGraph, TaskKey, TaskNode};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

const STATE_VERSION: u8 = 1;
const STATE_DIR: &str = ".pnpm-task-run-state-v1";
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

pub struct TaskRunStateContext {
    file_path: PathBuf,
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
        let file_path =
            workspace_dir.join("node_modules").join(STATE_DIR).join(format!("{invocation}.jsonl"));
        Self { file_path, invocation, keys_by_id, ids_by_key }
    }

    pub fn read_completed_tasks(&self) -> miette::Result<Option<HashSet<TaskKey>>> {
        let contents = match fs::read(&self.file_path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("reading {}", self.file_path.display()));
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
        if header.version != STATE_VERSION || header.invocation != self.invocation {
            return Ok(None);
        }
        let mut completed = HashSet::new();
        for line in lines {
            let Ok(record) = serde_json::from_str::<TaskRecord>(line) else { return Ok(None) };
            if record.run != header.run {
                continue;
            }
            let id = TaskId { project: record.project, task: record.task };
            let Some(key) = self.keys_by_id.get(&id) else { return Ok(None) };
            completed.insert(key.clone());
        }
        Ok(Some(completed))
    }

    pub fn start(&self, completed_tasks: &HashSet<TaskKey>) -> miette::Result<TaskRunState> {
        let run = run_id();
        let header = StateHeader {
            version: STATE_VERSION,
            invocation: self.invocation.clone(),
            run: run.clone(),
        };
        let mut completed: Vec<&TaskId> =
            completed_tasks.iter().map(|key| &self.ids_by_key[key]).collect();
        completed.sort();
        let mut contents = serde_json::to_string(&header).expect("task state header serializes");
        contents.push('\n');
        for id in &completed {
            let record =
                TaskRecord { run: run.clone(), project: id.project.clone(), task: id.task.clone() };
            contents.push_str(&serde_json::to_string(&record).expect("task record serializes"));
            contents.push('\n');
        }
        let state_dir = self.file_path.parent().expect("task state file has a parent");
        fs::create_dir_all(state_dir)
            .into_diagnostic()
            .wrap_err_with(|| format!("creating {}", state_dir.display()))?;
        pnpm_fs::write_atomic(&self.file_path, contents.as_bytes())
            .into_diagnostic()
            .wrap_err_with(|| format!("writing {}", self.file_path.display()))?;
        // Only the latest recursive invocation is resumable. Otherwise an
        // old compatible journal could become active after intervening work.
        for entry in fs::read_dir(state_dir)
            .into_diagnostic()
            .wrap_err_with(|| format!("reading {}", state_dir.display()))?
        {
            let entry = entry
                .into_diagnostic()
                .wrap_err_with(|| format!("reading {}", state_dir.display()))?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let file_type = entry
                .file_type()
                .into_diagnostic()
                .wrap_err_with(|| format!("reading stale task state {name}"))?;
            if entry.path() != self.file_path && is_state_file_name(&name) && !file_type.is_dir() {
                match fs::remove_file(entry.path()) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(error)
                            .into_diagnostic()
                            .wrap_err_with(|| format!("removing stale task state {name}"));
                    }
                }
            }
        }
        let file = OpenOptions::new()
            .append(true)
            .open(&self.file_path)
            .into_diagnostic()
            .wrap_err_with(|| format!("opening {}", self.file_path.display()))?;
        Ok(TaskRunState {
            file_path: self.file_path.clone(),
            writer: Mutex::new(TaskRunStateWriter {
                file: Some(file),
                run,
                completed: completed_tasks.clone(),
            }),
        })
    }
}

pub struct TaskRunState {
    file_path: PathBuf,
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
        if !writer.completed.insert(key.clone()) {
            return Ok(());
        }
        let id = task_id(node, workspace_dir);
        let record = TaskRecord { run: writer.run.clone(), project: id.project, task: id.task };
        let line = serde_json::to_string(&record).expect("task record serializes");
        let file = writer.file.as_mut().expect("unfinished task state has an open file");
        writeln!(file, "{line}")
            .into_diagnostic()
            .wrap_err_with(|| format!("writing {}", self.file_path.display()))
    }

    pub fn finish(&self) -> miette::Result<()> {
        let file = self.writer.lock().expect("task state lock is not poisoned").file.take();
        drop(file);
        match fs::remove_file(&self.file_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
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

fn is_state_file_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() == 70
        && &bytes[64..] == b".jsonl"
        && bytes[..64].iter().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn run_id() -> String {
    let timestamp =
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_nanos());
    let sequence = RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{}-{timestamp}-{sequence}", std::process::id())
}

#[cfg(test)]
mod tests;
