use super::{
    Arc, Command, Glob, IntoDiagnostic, Path, ProjectInputHashes, TaskCache, TaskKeyInputs,
    TaskNode, TaskSettings, check_input_directories, create_hex_hash, create_hex_hash_bytes,
    create_hex_hash_from_file, env, fs, io,
};
use wax::Program;

#[derive(Debug)]
pub(super) struct HashedFile {
    pub(super) rel_path: String,
    pub(super) hash: String,
}

/// The globs a task's `inputs` declaration replaces the default set with,
/// and the `+`-prefixed ones that add to it.
fn input_globs(inputs: Option<&[String]>) -> miette::Result<(Vec<Glob<'_>>, Vec<Glob<'static>>)> {
    let Some(patterns) = inputs else {
        return Ok((Vec::new(), Vec::new()));
    };
    let (add, replace): (Vec<&String>, Vec<&String>) =
        patterns.iter().partition(|pattern| pattern.starts_with('+'));
    Ok((
        compile_globs_ref(&replace)?,
        compile_globs_owned(
            &add.iter().map(|pattern| pattern[1..].to_string()).collect::<Vec<_>>(),
        )?,
    ))
}

/// A tracked input's contribution to the key, or `None` when the file is
/// gone by the time it is hashed.
///
/// A symlink contributes its target path the way git records it, rather
/// than the content it points at: following the link would hash a file
/// the project does not own, and a link that dangles has nothing to
/// hash at all. The `symlink:` tag keeps a link apart from a regular
/// file that happens to hold the same bytes.
fn hash_input(path: &Path) -> io::Result<Option<String>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() {
        return match fs::read_link(path) {
            Ok(target) => {
                let target = create_hex_hash_bytes(target.as_os_str().as_encoded_bytes());
                Ok(Some(format!("symlink:{target}")))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        };
    }
    match create_hex_hash_from_file(path) {
        Ok(hash) => Ok(Some(hash)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) fn compile_globs(patterns: &[String]) -> miette::Result<Vec<Glob<'_>>> {
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

fn has_submodule_inputs(project: &Path) -> miette::Result<bool> {
    let superproject =
        git_input_metadata(project, &["rev-parse", "--show-superproject-working-tree"])?;
    if superproject.iter().any(|byte| !byte.is_ascii_whitespace()) {
        return Ok(true);
    }
    let index = git_input_metadata(project, &["ls-files", "--stage", "-z"])?;
    Ok(index.split(|byte| *byte == 0).any(|entry| entry.starts_with(b"160000 ")))
}

fn git_input_metadata(project: &Path, args: &[&str]) -> miette::Result<Vec<u8>> {
    let project_display = project.display();
    let output = Command::new("git").args(args).current_dir(project).output().map_err(|error| {
        miette::miette!("reading Git input metadata in {project_display}: {error}")
    })?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(miette::miette!("reading Git input metadata in {project_display}: {error}"));
    }
    Ok(output.stdout)
}

fn tracked_input_files(project: &Path) -> miette::Result<std::process::Output> {
    let project_display = project.display();
    let output = Command::new("git")
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
        .current_dir(project)
        .output()
        .map_err(|error| miette::miette!("running git ls-files in {project_display}: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        return Err(miette::miette!("git ls-files failed in {project_display}: {stderr}"));
    }
    Ok(output)
}

fn hash_tracked_inputs(project: &Path) -> miette::Result<Vec<HashedFile>> {
    let project_display = project.display();
    let output = tracked_input_files(project)?;
    let mut files: Vec<HashedFile> = Vec::new();
    for rel_path in output.stdout.split(|byte| *byte == 0) {
        if rel_path.is_empty() {
            continue;
        }
        let rel_path = std::str::from_utf8(rel_path).map_err(|error| {
            let display_path = String::from_utf8_lossy(rel_path);
            miette::miette!(
                "non-UTF-8 cache input path {display_path:?} in {project_display}: {error}",
            )
        })?;
        if rel_path == "node_modules" || rel_path.starts_with("node_modules/") {
            continue;
        }
        check_input_directories(project, Path::new(rel_path)).into_diagnostic()?;
        let hash = hash_input(&project.join(rel_path)).map_err(|error| {
            miette::miette!("hashing cache input {rel_path:?} in {project_display}: {error}")
        })?;
        let Some(hash) = hash else { continue };
        files.push(HashedFile { rel_path: rel_path.to_string(), hash });
    }
    Ok(files)
}

impl TaskCache {
    /// The task's cache key: `pnpm-pipeline-task:v0` over the components
    /// the RFC names, NUL-separated and hashed.
    pub fn compute_task_key(&self, inputs: &TaskKeyInputs<'_>) -> miette::Result<Option<String>> {
        let mut components: Vec<String> = vec![
            "pnpm-pipeline-task:v1".to_string(),
            format!("platform:{}:{}", env::consts::OS, env::consts::ARCH),
            format!("outputs:{:?}", inputs.settings.and_then(|settings| settings.outputs.as_ref())),
            self.project_rel(&inputs.node.project),
            inputs.node.task_name.clone(),
            format!("lockfile:{}", self.lockfile_hash),
            format!("runtime:{}", self.runtime_fingerprint),
        ];
        for (stage, body) in inputs.script_bodies {
            components.push(format!("script:{stage}={body}"));
        }
        for name in inputs.settings.and_then(|settings| settings.env.as_deref()).unwrap_or_default()
        {
            let value = inputs.environment.get(name).cloned().or_else(|| env::var(name).ok());
            components.push(format!("env:{name}={value:?}"));
        }
        let Some(files) = self.input_files(inputs.node, inputs.settings)? else { return Ok(None) };
        for file in files.iter() {
            components.push(format!("file:{}={}", file.rel_path, file.hash));
        }
        for dependency_key in inputs.dependency_keys {
            components.push(format!("dep:{dependency_key}"));
        }
        Ok(Some(create_hex_hash(&components.join("\0"))))
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
    ) -> miette::Result<ProjectInputHashes> {
        let Some(all) = self.hashed_project_files(&node.project)? else { return Ok(None) };
        let output_globs = compile_globs(
            settings.and_then(|settings| settings.outputs.as_deref()).unwrap_or_default(),
        )?;
        let (replace_globs, add_globs) =
            input_globs(settings.and_then(|settings| settings.inputs.as_deref()))?;
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
        Ok(Some(Arc::new(filtered)))
    }

    pub(super) fn hashed_project_files(
        &self,
        project: &Path,
    ) -> miette::Result<ProjectInputHashes> {
        if let Some(files) =
            self.project_files.lock().expect("project-files lock is not poisoned").get(project)
        {
            return Ok(files.as_ref().map(Arc::clone));
        }
        if has_submodule_inputs(project)? {
            self.project_files
                .lock()
                .expect("project-files lock is not poisoned")
                .insert(project.to_path_buf(), None);
            return Ok(None);
        }
        let mut files = hash_tracked_inputs(project)?;
        files.sort_by(|left, right| left.rel_path.cmp(&right.rel_path));
        let files = Arc::new(files);
        self.project_files
            .lock()
            .expect("project-files lock is not poisoned")
            .insert(project.to_path_buf(), Some(Arc::clone(&files)));
        Ok(Some(files))
    }
}
