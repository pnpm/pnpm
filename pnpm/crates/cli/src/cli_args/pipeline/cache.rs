//! The local task cache: content-addressed task results under pnpm's
//! cache directory. A proof-of-concept tier of the workspace task cache
//! RFC (pnpm/rfcs#22): keys cover the script text, the declared env
//! values, the project's tracked files, the upstream task keys, the
//! lockfile, and the runtime — with the lockfile hashed whole as a
//! deliberate `PoC` stand-in for the per-importer dependency-graph hash the
//! RFC specifies.

use super::{
    capture::CapturedScript,
    paths::{check_ancestors, validate_relative_path},
};
use pnpm_config::TaskSettings;
use pnpm_crypto_hash::{create_hex_hash, create_hex_hash_from_file, create_short_hash};
use pnpm_workspace_task_scheduler::TaskNode;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};
use wax::{
    Glob, Program,
    walk::{Entry, FileIterator},
};

/// How a task met the cache, for the run report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheDisposition {
    Hit,
    Miss,
    /// The task is not cacheable (no declared `outputs`, `cache: false`,
    /// or `--no-cache`).
    Bypass,
}

/// One stored task: its captured output streams and the output files it
/// produced, relative to the project directory.
#[derive(Debug, Serialize, Deserialize)]
pub struct StoredTask {
    pub version: u32,
    pub task: String,
    pub files: Vec<String>,
    pub hashes: HashMap<String, String>,
    pub scripts: Vec<CapturedScript>,
    /// Where the entry's `outputs/` tree lives; not serialized.
    #[serde(skip)]
    pub entry_dir: PathBuf,
}

pub struct TaskCache {
    tasks_dir: PathBuf,
    state_dir: PathBuf,
    workspace_root: PathBuf,
    lockfile_hash: String,
    runtime_fingerprint: String,
    /// Per-project tracked-file hashes, shared by every task of the
    /// project: enumeration and hashing run once, per-task input specs
    /// filter the shared list.
    project_files: Mutex<HashMap<PathBuf, Arc<Vec<HashedFile>>>>,
}

#[derive(Debug)]
struct HashedFile {
    rel_path: String,
    hash: String,
}

/// One entry of a task's last-outputs record: a file the previous run or
/// restore produced, with the content it left behind.
#[derive(Debug, Serialize, Deserialize)]
struct RecordedFile {
    path: String,
    hash: String,
}

pub struct TaskKeyInputs<'a> {
    pub node: &'a TaskNode,
    pub settings: Option<&'a TaskSettings>,
    /// The cache keys of the tasks this task depends on, sorted.
    pub dependency_keys: &'a [&'a str],
    /// `(stage, body)` of every script the task runs, in run order.
    pub environment: &'a HashMap<String, String>,
    pub script_bodies: &'a [(String, String)],
}

impl TaskCache {
    pub fn open(data_dir: &Path, workspace_root: &Path) -> miette::Result<TaskCache> {
        let tasks_dir = data_dir.join("tasks");
        let state_dir = data_dir.join("state");
        fs::create_dir_all(&tasks_dir)
            .and_then(|()| fs::create_dir_all(&state_dir))
            .map_err(|error| miette::miette!("creating the pipeline cache directories: {error}"))?;
        let lockfile_hash = create_hex_hash_from_file(&workspace_root.join("pnpm-lock.yaml"))
            .unwrap_or_else(|_| "no-lockfile".to_string());
        // Resolved from the workspace, not the invoking process: with
        // context-aware toolchains (pnpm's own shims included) the
        // runtime is a function of the directory, and a `--dir`
        // invocation must fingerprint the runtime the workspace's
        // scripts will actually get.
        let runtime_fingerprint = Command::new("node")
            .arg("--version")
            .current_dir(workspace_root)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map_or_else(
                || "no-node".to_string(),
                |output| String::from_utf8_lossy(&output.stdout).trim().to_string(),
            );
        Ok(TaskCache {
            tasks_dir,
            state_dir,
            workspace_root: workspace_root.to_path_buf(),
            lockfile_hash,
            runtime_fingerprint,
            project_files: Mutex::new(HashMap::new()),
        })
    }

    /// The task's cache key: `pnpm-pipeline-task:v0` over the components
    /// the RFC names, NUL-separated and hashed.
    pub fn compute_task_key(&self, inputs: &TaskKeyInputs<'_>) -> miette::Result<String> {
        let TaskKeyInputs { node, settings, dependency_keys, script_bodies, environment } = *inputs;
        let project_rel = self.project_rel(&node.project);
        let mut components: Vec<String> = vec![
            "pnpm-pipeline-task:v1".to_string(),
            format!("platform:{}:{}", env::consts::OS, env::consts::ARCH),
            format!("outputs:{:?}", settings.and_then(|settings| settings.outputs.as_ref())),
            project_rel,
            node.task_name.clone(),
            format!("lockfile:{}", self.lockfile_hash),
            format!("runtime:{}", self.runtime_fingerprint),
        ];
        for (stage, body) in script_bodies {
            components.push(format!("script:{stage}={body}"));
        }
        for name in settings.and_then(|settings| settings.env.as_deref()).unwrap_or_default() {
            let value = environment.get(name).cloned().or_else(|| env::var(name).ok());
            components.push(format!("env:{name}={value:?}"));
        }
        for file in self.input_files(node, settings)?.iter() {
            components.push(format!("file:{}={}", file.rel_path, file.hash));
        }
        for dependency_key in dependency_keys {
            components.push(format!("dep:{dependency_key}"));
        }
        Ok(create_hex_hash(&components.join("\0")))
    }

    pub fn lookup(&self, key: &str) -> Option<StoredTask> {
        let entry_dir = self.entry_dir(key);
        check_ancestors(&self.tasks_dir, entry_dir.strip_prefix(&self.tasks_dir).ok()?).ok()?;
        let meta = fs::read_to_string(entry_dir.join("meta.json")).ok()?;
        let mut stored: StoredTask = serde_json::from_str(&meta).ok()?;
        if stored.version != 2 {
            return None;
        }
        stored.entry_dir = entry_dir;
        Some(stored)
    }

    /// Restore a stored task into the working tree. `Err` carries the
    /// human-readable reason the restore refused — the caller runs the
    /// task normally.
    ///
    /// A file is only ever overwritten or deleted when its current
    /// content matches what the previous run or restore left there
    /// (or what the artifact would write anyway). The record therefore
    /// carries content hashes, not just paths — a path set cannot tell
    /// "we produced it" apart from "we produced it and the user has
    /// edited it since".
    pub fn restore(
        &self,
        stored: &StoredTask,
        project_dir: &Path,
        task_id: &str,
    ) -> Result<(), String> {
        let previous = self.read_output_record(task_id);
        let validate = |root: &Path, relative: &str| {
            validate_relative_path(Path::new(relative))
                .and_then(|()| check_ancestors(root, Path::new(relative)))
                .map_err(|error| error.to_string())
        };
        for relative in stored
            .files
            .iter()
            .map(String::as_str)
            .chain(previous.iter().map(|record| record.path.as_str()))
        {
            validate(project_dir, relative)?;
        }
        for relative in &stored.files {
            validate(&stored.entry_dir, &format!("outputs/{relative}"))?;
            let actual =
                create_hex_hash_from_file(&stored.entry_dir.join("outputs").join(relative))
                    .map_err(|error| error.to_string())?;
            if stored.hashes.get(relative) != Some(&actual) {
                return Err(format!("cached output failed integrity: {relative}"));
            }
        }
        for rel_path in &stored.files {
            let target = project_dir.join(rel_path);
            if !target.exists() {
                continue;
            }
            let target_hash = create_hex_hash_from_file(&target).unwrap_or_default();
            let ours = previous
                .iter()
                .any(|recorded| recorded.path == *rel_path && recorded.hash == target_hash);
            if ours {
                continue;
            }
            let artifact_hash =
                create_hex_hash_from_file(&stored.entry_dir.join("outputs").join(rel_path))
                    .unwrap_or_else(|_| "unreadable".to_string());
            if target_hash != artifact_hash {
                return Err(format!(
                    "{rel_path} in the working tree is not what the previous run produced",
                ));
            }
        }
        // What the previous run produced and this artifact does not is
        // stale output; restoring only additions would leave a mixture of
        // two builds. A stale file the user has edited since is theirs
        // now, so the restore refuses rather than deleting it.
        let mut stale: Vec<&RecordedFile> = Vec::new();
        for recorded in &previous {
            if stored.files.contains(&recorded.path) {
                continue;
            }
            let target = project_dir.join(&recorded.path);
            if !target.exists() {
                continue;
            }
            if create_hex_hash_from_file(&target).unwrap_or_default() != recorded.hash {
                return Err(format!(
                    "{} was modified after the previous run produced it",
                    recorded.path,
                ));
            }
            stale.push(recorded);
        }
        for recorded in stale {
            validate(project_dir, &recorded.path)?;
            fs::remove_file(project_dir.join(&recorded.path)).map_err(|error| error.to_string())?;
        }
        let mut record: Vec<RecordedFile> = Vec::with_capacity(stored.files.len());
        for rel_path in &stored.files {
            let source = stored.entry_dir.join("outputs").join(rel_path);
            let target = project_dir.join(rel_path);
            validate(project_dir, rel_path)?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            if let Err(error) = fs::copy(&source, &target) {
                return Err(format!("copying {rel_path}: {error}"));
            }
            record.push(RecordedFile {
                path: rel_path.clone(),
                hash: create_hex_hash_from_file(&source).unwrap_or_default(),
            });
        }
        self.write_output_record(task_id, &record).map_err(|error| error.to_string())
    }

    /// Store a successful task: its declared outputs and captured logs.
    pub fn store(
        &self,
        key: &str,
        project_dir: &Path,
        task_id: &str,
        outputs: &[String],
        scripts: Vec<CapturedScript>,
    ) -> io::Result<()> {
        let files = collect_output_files(project_dir, outputs)?;
        let entry_dir = self.entry_dir(key);
        let parent = entry_dir.parent().expect("cache entry parent");
        fs::create_dir_all(parent)?;
        let staging = tempfile::Builder::new().prefix(".publish-").tempdir_in(parent)?;
        let staging_dir = staging.path();
        let mut record: Vec<RecordedFile> = Vec::with_capacity(files.len());
        for rel_path in &files {
            let target = staging_dir.join("outputs").join(rel_path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            validate_relative_path(Path::new(rel_path))?;
            check_ancestors(project_dir, Path::new(rel_path))?;
            fs::copy(project_dir.join(rel_path), &target)?;
            record.push(RecordedFile {
                path: rel_path.clone(),
                hash: create_hex_hash_from_file(&target)?,
            });
        }
        let meta = StoredTask {
            version: 2,
            hashes: record.iter().map(|file| (file.path.clone(), file.hash.clone())).collect(),
            task: task_id.to_string(),
            files,
            scripts,
            entry_dir: PathBuf::new(),
        };
        fs::write(staging_dir.join("meta.json"), serde_json::to_vec_pretty(&meta)?)?;
        match fs::rename(staging_dir, &entry_dir) {
            Ok(()) => {}
            Err(error) => {
                let destination_exists = matches!(
                    error.kind(),
                    io::ErrorKind::AlreadyExists | io::ErrorKind::DirectoryNotEmpty,
                ) || cfg!(windows)
                    && error.kind() == io::ErrorKind::PermissionDenied;
                // Windows can report access denied when a destination directory already exists.
                if !destination_exists || !self.matches_snapshot(key, &meta)? {
                    return Err(error);
                }
            }
        }
        self.write_output_record(task_id, &record)
    }

    fn matches_snapshot(&self, key: &str, expected: &StoredTask) -> io::Result<bool> {
        let Some(stored) = self.lookup(key) else {
            return Ok(false);
        };
        if stored.files != expected.files || stored.hashes != expected.hashes {
            return Ok(false);
        }
        for relative in &stored.files {
            check_ancestors(&stored.entry_dir, &Path::new("outputs").join(relative))?;
            if create_hex_hash_from_file(&stored.entry_dir.join("outputs").join(relative))?
                != stored.hashes[relative]
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn entry_dir(&self, key: &str) -> PathBuf {
        self.tasks_dir.join(&key[..2]).join(key)
    }

    fn output_record_path(&self, task_id: &str) -> PathBuf {
        self.state_dir.join(format!("{}.json", create_short_hash(task_id)))
    }

    fn read_output_record(&self, task_id: &str) -> Vec<RecordedFile> {
        fs::read_to_string(self.output_record_path(task_id))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn write_output_record(&self, task_id: &str, files: &[RecordedFile]) -> io::Result<()> {
        let contents = serde_json::to_vec(files)?;
        pnpm_fs::write_atomic(&self.output_record_path(task_id), &contents)
    }

    fn project_rel(&self, project: &Path) -> String {
        pathdiff::diff_paths(project, &self.workspace_root)
            .map(|path| path.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"))
            .filter(|path| !path.is_empty())
            .unwrap_or_else(|| ".".to_string())
    }

    /// The task's input files with their content hashes: the project's
    /// tracked (and untracked, unignored) files minus its declared
    /// outputs and `node_modules`, narrowed by the task's `inputs` globs
    /// when declared (`+`-prefixed entries add to the default set
    /// instead).
    fn input_files(
        &self,
        node: &TaskNode,
        settings: Option<&TaskSettings>,
    ) -> miette::Result<Arc<Vec<HashedFile>>> {
        let all = self.hashed_project_files(&node.project)?;
        let outputs = settings.and_then(|settings| settings.outputs.as_deref()).unwrap_or_default();
        let inputs = settings.and_then(|settings| settings.inputs.as_deref());
        let output_globs = compile_globs(outputs)?;
        let (replace_globs, add_globs) = match inputs {
            None => (Vec::new(), Vec::new()),
            Some(patterns) => {
                let (add, replace): (Vec<&String>, Vec<&String>) =
                    patterns.iter().partition(|pattern| pattern.starts_with('+'));
                (
                    compile_globs_ref(&replace)?,
                    compile_globs_owned(
                        &add.iter().map(|pattern| pattern[1..].to_string()).collect::<Vec<_>>(),
                    )?,
                )
            }
        };
        let filtered: Vec<HashedFile> = all
            .iter()
            .filter(|file| !output_globs.iter().any(|glob| glob.is_match(file.rel_path.as_str())))
            .filter(|file| {
                let in_default = replace_globs.is_empty()
                    || replace_globs.iter().any(|glob| glob.is_match(file.rel_path.as_str()));
                in_default || add_globs.iter().any(|glob| glob.is_match(file.rel_path.as_str()))
            })
            .map(|file| HashedFile { rel_path: file.rel_path.clone(), hash: file.hash.clone() })
            .collect();
        Ok(Arc::new(filtered))
    }

    fn hashed_project_files(&self, project: &Path) -> miette::Result<Arc<Vec<HashedFile>>> {
        if let Some(files) =
            self.project_files.lock().expect("project-files lock is not poisoned").get(project)
        {
            return Ok(Arc::clone(files));
        }
        let project_display = project.display();
        let output = Command::new("git")
            .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
            .current_dir(project)
            .output()
            .map_err(|error| {
                miette::miette!("running git ls-files in {project_display}: {error}")
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr = stderr.trim();
            return Err(miette::miette!("git ls-files failed in {project_display}: {stderr}"));
        }
        let mut files: Vec<HashedFile> = Vec::new();
        for rel_path in output.stdout.split(|byte| *byte == 0) {
            if rel_path.is_empty() {
                continue;
            }
            let rel_path = String::from_utf8_lossy(rel_path).replace('\\', "/");
            if rel_path == "node_modules" || rel_path.starts_with("node_modules/") {
                continue;
            }
            // A tracked file deleted from the working tree contributes
            // nothing; the deletion shows up as the file's absence.
            let Ok(hash) = create_hex_hash_from_file(&project.join(&rel_path)) else {
                continue;
            };
            files.push(HashedFile { rel_path, hash });
        }
        files.sort_by(|left, right| left.rel_path.cmp(&right.rel_path));
        let files = Arc::new(files);
        self.project_files
            .lock()
            .expect("project-files lock is not poisoned")
            .insert(project.to_path_buf(), Arc::clone(&files));
        Ok(files)
    }
}

/// The files under `project_dir` the `outputs` globs match, as sorted
/// `/`-separated relative paths. `node_modules` and `.git` are never
/// walked.
fn collect_output_files(project_dir: &Path, outputs: &[String]) -> io::Result<Vec<String>> {
    let globs = compile_globs(outputs)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    if globs.is_empty() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for glob in globs {
        for entry in glob
            .walk(project_dir)
            .not(wax::any(["**/node_modules/**", "**/.git/**"]))
            .map_err(io::Error::other)?
        {
            let entry = entry.map_err(io::Error::other)?;
            if entry.file_type().is_file() {
                let relative = entry
                    .path()
                    .strip_prefix(project_dir)
                    .map_err(|_| io::Error::other("output glob must stay inside the project"))?
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                validate_relative_path(Path::new(&relative))?;
                check_ancestors(project_dir, Path::new(&relative))?;
                files.push(relative);
            }
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn compile_globs(patterns: &[String]) -> miette::Result<Vec<Glob<'_>>> {
    patterns
        .iter()
        .map(|pattern| {
            Glob::new(pattern).map_err(|error| miette::miette!("invalid glob {pattern:?}: {error}"))
        })
        .collect()
}

fn compile_globs_ref<'a>(patterns: &[&'a String]) -> miette::Result<Vec<Glob<'a>>> {
    patterns
        .iter()
        .map(|pattern| {
            Glob::new(pattern).map_err(|error| miette::miette!("invalid glob {pattern:?}: {error}"))
        })
        .collect()
}

fn compile_globs_owned(patterns: &[String]) -> miette::Result<Vec<Glob<'static>>> {
    patterns
        .iter()
        .map(|pattern| {
            Glob::new(pattern)
                .map(Glob::into_owned)
                .map_err(|error| miette::miette!("invalid glob {pattern:?}: {error}"))
        })
        .collect()
}

#[cfg(test)]
mod tests;
