mod globs;

use super::{
    Arc, Command, FileIterator, Glob, InputsUnavailable, IntoDiagnostic, Path, PathBuf,
    ProjectFiles, ProjectInputHashes, TaskCache, TaskKeyInputs, TaskNode, TaskSettings,
    check_ancestors, check_input_directories, create_hex_hash, create_hex_hash_bytes,
    create_hex_hash_from_file, env, fs, io, validate_relative_path,
};
use globs::{ConfiguredInputs, InputSelection, parse_inputs, walk_additive_inputs};
use wax::Program;

#[derive(Debug, Clone)]
pub(super) struct HashedFile {
    pub(super) rel_path: String,
    pub(super) hash: String,
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
            let glob = Glob::new(pattern)
                .map_err(|error| miette::miette!("invalid glob {pattern:?}: {error}"))?;
            if !matches!(glob.has_root(), wax::query::When::Never) {
                return Err(miette::miette!("cache glob must be relative: {pattern:?}"));
            }
            Ok(glob)
        })
        .collect()
}

fn has_submodule_inputs(project: &Path) -> miette::Result<bool> {
    let superproject =
        git_input_metadata(project, &["rev-parse", "--show-superproject-working-tree"])?;
    if superproject
        .iter()
        .any(|byte| !byte.is_ascii_whitespace())
    {
        return Ok(true);
    }
    let index = git_input_metadata(project, &["ls-files", "--stage", "-z"])?;
    Ok(index
        .split(|byte| *byte == 0)
        .any(|entry| entry.starts_with(b"160000 ")))
}

fn git_input_metadata(project: &Path, args: &[&str]) -> miette::Result<Vec<u8>> {
    let project_display = project.display();
    let output = Command::new("git")
        .args(args)
        .current_dir(project)
        .output()
        .map_err(|error| {
            miette::miette!("reading Git input metadata in {project_display}: {error}")
        })?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(miette::miette!("reading Git input metadata in {project_display}: {error}"));
    }
    Ok(output.stdout)
}

fn tracked_input_files(project: &Path) -> miette::Result<crate::process::Output> {
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
        let rel_path = std::str::from_utf8(rel_path)
            .map_err(|error| {
                let display_path = String::from_utf8_lossy(rel_path);
                miette::miette!(
                    "non-UTF-8 cache input path {display_path:?} in {project_display}: {error}",
                )
            })?;
        if rel_path == "node_modules" || rel_path.starts_with("node_modules/") {
            continue;
        }
        check_input_directories(project, Path::new(rel_path)).into_diagnostic()?;
        let hash = hash_input(&project.join(rel_path))
            .map_err(|error| {
                miette::miette!("hashing cache input {rel_path:?} in {project_display}: {error}")
            })?;
        let Some(hash) = hash else { continue };
        files.push(HashedFile { rel_path: rel_path.to_string(), hash });
    }
    Ok(files)
}

impl TaskCache {
    /// The task's cache key: `pnpm-pipeline-task:v1` over the components
    /// the RFC names, NUL-separated and hashed.
    pub fn compute_task_key(&self, inputs: &TaskKeyInputs<'_>) -> miette::Result<Option<String>> {
        let lockfile_hash = self.lockfile_hash(inputs.lockfile_path);
        let mut components: Vec<String> = vec![
            "pnpm-pipeline-task:v1".to_string(),
            platform_cache_component(),
            format!("outputs:{:?}", inputs.settings.and_then(|settings| settings.outputs.as_ref())),
            self.project_rel(&inputs.node.project),
            inputs.node.task_name.clone(),
            format!("lockfile:{lockfile_hash}"),
            format!("runtime:{}", self.runtime_fingerprint),
        ];
        for (stage, body) in inputs.script_bodies {
            components.push(format!("script:{stage}={body}"));
        }
        for name in inputs.settings.and_then(|settings| settings.env.as_deref()).unwrap_or_default()
        {
            let value = inputs.environment
                .get(name)
                .cloned()
                .or_else(|| env::var(name).ok());
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

    pub(super) fn lockfile_hash(&self, lockfile_path: &Path) -> String {
        if let Some(hash) = self.lockfile_hashes
            .lock()
            .expect("lockfile-hashes lock is not poisoned")
            .get(lockfile_path)
        {
            return hash.clone();
        }
        let hash =
            create_hex_hash_from_file(lockfile_path).unwrap_or_else(|_| "no-lockfile".to_string());
        self.lockfile_hashes
            .lock()
            .expect("lockfile-hashes lock is not poisoned")
            .insert(lockfile_path.to_path_buf(), hash.clone());
        hash
    }

    pub fn project_rel(&self, project: &Path) -> String {
        pathdiff::diff_paths(project, &self.workspace_root)
            .map(|path| path.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"))
            .filter(|path| !path.is_empty())
            .unwrap_or_else(|| ".".to_string())
    }

    /// The task's input files with their content hashes: the project's
    /// tracked (and untracked, unignored) files minus its declared
    /// outputs and `node_modules`, narrowed or extended by the task's
    /// `inputs` globs when declared.
    fn input_files(
        &self,
        node: &TaskNode,
        settings: Option<&TaskSettings>,
    ) -> miette::Result<ProjectInputHashes> {
        let selection =
            parse_inputs(settings.and_then(|settings| settings.inputs.as_deref()))?;
        if matches!(selection, InputSelection::Empty) {
            check_ancestors(&node.project, Path::new("")).into_diagnostic()?;
            return Ok(Some(Arc::new(Vec::new())));
        }
        let Some(all) = self.hashed_project_files(&node.project)? else { return Ok(None) };
        let output_globs = compile_globs(
            settings.and_then(|settings| settings.outputs.as_deref()).unwrap_or_default(),
        )?;
        let files = match selection {
            InputSelection::Default => filter_default_inputs(&all, &output_globs),
            InputSelection::Empty => Vec::new(),
            InputSelection::Configured(configured) => resolve_configured_inputs(
                &self.workspace_root,
                &node.project,
                &all,
                &output_globs,
                &configured,
            )?,
        };
        Ok(Some(Arc::new(files)))
    }

    pub(super) fn hashed_project_files(
        &self,
        project: &Path,
    ) -> miette::Result<ProjectInputHashes> {
        if let Some(files) = self.project_files
            .lock()
            .expect("project-files lock is not poisoned")
            .get(project)
        {
            return Ok(files.hashes());
        }
        let files = project_files(project)?;
        let hashes = files.hashes();
        self.project_files
            .lock()
            .expect("project-files lock is not poisoned")
            .insert(project.to_path_buf(), files);
        Ok(hashes)
    }

    /// Why the project has no file set, when it has none.
    pub fn inputs_unavailable(&self, project: &Path) -> Option<InputsUnavailable> {
        let cached = self.project_files.lock().expect("project-files lock is not poisoned");
        match cached.get(project) {
            Some(ProjectFiles::Unavailable(reason)) => Some(*reason),
            _ => None,
        }
    }
}

fn filter_default_inputs(all: &[HashedFile], output_globs: &[Glob<'_>]) -> Vec<HashedFile> {
    all.iter()
        .filter(|file| !matches_project_output(&file.rel_path, output_globs))
        .cloned()
        .collect()
}

fn resolve_configured_inputs(
    workspace_root: &Path,
    project: &Path,
    all: &[HashedFile],
    output_globs: &[Glob<'_>],
    configured: &ConfiguredInputs<'_>,
) -> miette::Result<Vec<HashedFile>> {
    let mut files: Vec<HashedFile> = all
        .iter()
        .filter(|file| {
            configured.replace.is_empty()
                || configured.replace
                    .iter()
                    .any(|glob| glob.is_match(file.rel_path.as_str()))
        })
        .cloned()
        .collect();
    files.extend(walk_additive_inputs(workspace_root, project, &configured.add)?);
    files.sort_by(|left, right| left.rel_path.cmp(&right.rel_path));
    files.dedup_by(|left, right| left.rel_path == right.rel_path);
    files.retain(|file| {
        !matches_project_output(&file.rel_path, output_globs)
            && !configured.exclude
                .iter()
                .any(|glob| glob.matches_rel_path(&file.rel_path))
    });
    Ok(files)
}

fn matches_project_output(rel_path: &str, output_globs: &[Glob<'_>]) -> bool {
    !rel_path.starts_with("../")
        && output_globs
            .iter()
            .any(|glob| glob.is_match(rel_path))
}

/// A project's file set, or why it has none.
fn project_files(project: &Path) -> miette::Result<ProjectFiles> {
    if !git_work_tree(project)? {
        return Ok(ProjectFiles::Unavailable(InputsUnavailable::NoGit));
    }
    if has_submodule_inputs(project)? {
        return Ok(ProjectFiles::Unavailable(InputsUnavailable::Submodules));
    }
    let mut files = hash_tracked_inputs(project)?;
    files.sort_by(|left, right| left.rel_path.cmp(&right.rel_path));
    Ok(ProjectFiles::Hashed(Arc::new(files)))
}

/// Whether git can enumerate the project's files at all.
///
/// The default input set is `git ls-files`, so a project outside a work
/// tree — or a machine without `git` — has no discoverable inputs. That is
/// a reason to run the task without a cache key, not to abort the whole
/// pipeline: the caller reports it and the task runs uncached.
fn git_work_tree(project: &Path) -> miette::Result<bool> {
    let project_display = project.display();
    match Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(project)
        .output()
    {
        Ok(output) => Ok(output.status.success() && output.stdout.trim_ascii() == b"true"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(miette::miette!("probing Git work tree in {project_display}: {error}")),
    }
}

fn platform_cache_component() -> String {
    #[cfg(not(target_family = "wasm"))]
    {
        format!("platform:{}:{}", env::consts::OS, env::consts::ARCH)
    }
    #[cfg(target_family = "wasm")]
    {
        let os = match pnpm_detect_libc::host_platform() {
            "darwin" => "macos",
            "win32" => "windows",
            "sunos" => "solaris",
            other => other,
        };
        format!("platform:{os}:{}", pnpm_detect_libc::host_target_arch())
    }
}
