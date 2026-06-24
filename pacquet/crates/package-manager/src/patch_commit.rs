use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_git_fetcher::PacklistError;
use pacquet_package_manifest::PackageManifestError;
use serde_json::{Map, Value};
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PkgFilesForDiff {
    Original(PathBuf),
    Temporary(PathBuf),
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PatchCommitError {
    #[display("Failed to read package manifest in {}: {source}", dir.display())]
    #[diagnostic(code(pacquet_package_manager::patch_commit_read_manifest))]
    ReadManifest {
        dir: PathBuf,
        #[error(source)]
        source: PackageManifestError,
    },

    #[display("Failed to compute package files for {}: {source}", dir.display())]
    #[diagnostic(code(pacquet_package_manager::patch_commit_packlist))]
    Packlist {
        dir: PathBuf,
        #[error(source)]
        source: PacklistError,
    },

    #[display("Failed to read directory {}: {source}", dir.display())]
    #[diagnostic(code(pacquet_package_manager::patch_commit_read_dir))]
    ReadDir {
        dir: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to create temporary patch directory {}: {source}", dir.display())]
    #[diagnostic(code(pacquet_package_manager::patch_commit_create_temp_dir))]
    CreateTempDir {
        dir: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to remove temporary patch directory {}: {source}", dir.display())]
    #[diagnostic(code(pacquet_package_manager::patch_commit_remove_temp_dir))]
    RemoveTempDir {
        dir: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display(
        "Failed to link package file from {} to {}: {source}",
        source_path.display(),
        target.display()
    )]
    #[diagnostic(code(pacquet_package_manager::patch_commit_link_file))]
    LinkFile {
        source_path: PathBuf,
        target: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Package file path escapes package directory: {path}")]
    #[diagnostic(code(pacquet_package_manager::patch_commit_invalid_package_file_path))]
    InvalidPackageFilePath { path: String },

    #[display(
        "Unable to diff directories. Make sure you have a recent version of 'git' available in PATH.\nThe following error was reported by 'git':\n{stderr}"
    )]
    #[diagnostic(code(pacquet_package_manager::patch_commit_diff_failed))]
    DiffFailed { stderr: String },

    #[display("Failed to run git diff: {source}")]
    #[diagnostic(code(pacquet_package_manager::patch_commit_diff_spawn))]
    DiffSpawn {
        #[error(source)]
        source: io::Error,
    },
}

pub fn prepare_pkg_files_for_diff(src: &Path) -> Result<PkgFilesForDiff, PatchCommitError> {
    prepare_pkg_files_for_diff_with_fs(src, &RealPatchCommitFs)
}

fn prepare_pkg_files_for_diff_with_fs(
    src: &Path,
    fs_ops: &impl PatchCommitFs,
) -> Result<PkgFilesForDiff, PatchCommitError> {
    let manifest = pacquet_package_manifest::safe_read_package_json_from_dir(src)
        .map_err(|source| PatchCommitError::ReadManifest { dir: src.to_path_buf(), source })?
        .unwrap_or_else(|| Value::Object(Map::default()));
    let files = pacquet_git_fetcher::packlist(src, &manifest)
        .map_err(|source| PatchCommitError::Packlist { dir: src.to_path_buf(), source })?;
    if all_files(src)? == files.iter().cloned().collect::<BTreeSet<_>>() {
        return Ok(PkgFilesForDiff::Original(src.to_path_buf()));
    }

    let temp_dir = temporary_filtered_dir(src);
    remove_existing_temp_dir_with_fs(&temp_dir, fs_ops)?;
    fs_ops
        .create_dir_all(&temp_dir)
        .map_err(|source| PatchCommitError::CreateTempDir { dir: temp_dir.clone(), source })?;
    for file in files {
        let relative_path = safe_package_file_path(&file)?;
        let source_path = src.join(&relative_path);
        let target = temp_dir.join(&relative_path);
        let parent =
            target.parent().expect("filtered package file target should have a parent directory");
        fs_ops.create_dir_all(parent).map_err(|source| PatchCommitError::CreateTempDir {
            dir: parent.to_path_buf(),
            source,
        })?;
        fs_ops.hard_link(&source_path, &target).map_err(|source| PatchCommitError::LinkFile {
            source_path,
            target,
            source,
        })?;
    }
    Ok(PkgFilesForDiff::Temporary(temp_dir))
}

pub fn diff_folders(folder_a: &Path, folder_b: &Path) -> Result<String, PatchCommitError> {
    let folder_a_slash = slash_path(folder_a);
    let folder_b_slash = slash_path(folder_b);
    let output = Command::new("git")
        .arg("-c")
        .arg("core.safecrlf=false")
        .arg("diff")
        .arg("--src-prefix=a/")
        .arg("--dst-prefix=b/")
        .arg("--ignore-cr-at-eol")
        .arg("--irreversible-delete")
        .arg("--full-index")
        .arg("--no-index")
        .arg("--text")
        .arg("--no-ext-diff")
        .arg("--no-color")
        .arg(&folder_a_slash)
        .arg(&folder_b_slash)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .map_err(|source| PatchCommitError::DiffSpawn { source })?;

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !stderr.is_empty() || !matches!(output.status.code(), Some(0 | 1)) {
        return Err(PatchCommitError::DiffFailed { stderr });
    }
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok(normalize_diff_output(&stdout, &folder_a_slash, &folder_b_slash))
}

fn remove_existing_temp_dir_with_fs(
    temp_dir: &Path,
    fs_ops: &impl PatchCommitFs,
) -> Result<(), PatchCommitError> {
    match fs_ops.remove_dir_all(temp_dir) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(PatchCommitError::RemoveTempDir { dir: temp_dir.to_path_buf(), source }),
    }
}

trait PatchCommitFs {
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    fn hard_link(&self, source: &Path, target: &Path) -> io::Result<()>;
    fn remove_dir_all(&self, path: &Path) -> io::Result<()>;
}

struct RealPatchCommitFs;

impl PatchCommitFs for RealPatchCommitFs {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }

    fn hard_link(&self, source: &Path, target: &Path) -> io::Result<()> {
        fs::hard_link(source, target)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::remove_dir_all(path)
    }
}

fn temporary_filtered_dir(src: &Path) -> PathBuf {
    let name = src
        .file_name()
        .map_or_else(|| String::from("patch"), |name| name.to_string_lossy().into_owned());
    src.parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{name}_tmp_{}", std::process::id()))
}

fn all_files(base: &Path) -> Result<BTreeSet<String>, PatchCommitError> {
    let mut files = BTreeSet::new();
    collect_files(base, base, &mut files)?;
    Ok(files)
}

fn collect_files(
    base: &Path,
    dir: &Path,
    files: &mut BTreeSet<String>,
) -> Result<(), PatchCommitError> {
    for entry in fs::read_dir(dir)
        .map_err(|source| PatchCommitError::ReadDir { dir: dir.to_path_buf(), source })?
    {
        let entry =
            entry.map_err(|source| PatchCommitError::ReadDir { dir: dir.to_path_buf(), source })?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|source| PatchCommitError::ReadDir { dir: path.clone(), source })?;
        if file_type.is_dir() {
            collect_files(base, &path, files)?;
        } else if file_type.is_file() {
            files.insert(relative_slash_path(base, &path));
        }
    }
    Ok(())
}

fn relative_slash_path(base: &Path, path: &Path) -> String {
    slash_path(path.strip_prefix(base).unwrap_or(path))
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn safe_package_file_path(path: &str) -> Result<PathBuf, PatchCommitError> {
    let path = path_from_forward_slash(path);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_),
            )
        })
    {
        return Err(PatchCommitError::InvalidPackageFilePath {
            path: path.to_string_lossy().into_owned(),
        });
    }
    Ok(path)
}

fn path_from_forward_slash(path: &str) -> PathBuf {
    path.split('/').collect()
}

fn normalize_diff_output(diff: &str, folder_a: &str, folder_b: &str) -> String {
    let mut out = String::with_capacity(diff.len());
    let mut in_hunk = false;
    for line in diff.split_inclusive('\n') {
        let (content, newline) = line.strip_suffix('\n').map_or((line, ""), |line| (line, "\n"));
        if content.starts_with("diff --git ") {
            in_hunk = false;
            out.push_str(&normalize_diff_path_line(content, folder_a, folder_b));
        } else if !in_hunk && is_diff_path_line(content) {
            out.push_str(&normalize_diff_path_line(content, folder_a, folder_b));
        } else {
            out.push_str(content);
        }
        if content.starts_with("@@ ") || content.starts_with("@@@ ") {
            in_hunk = true;
        }
        out.push_str(newline);
    }
    if let Some(without_marker) = out.strip_suffix("\n\\ No newline at end of file\n") {
        out = format!("{without_marker}\n");
    }
    remove_ds_store_diff_blocks(&out)
}

fn is_diff_path_line(line: &str) -> bool {
    line.starts_with("diff --git ") || line.starts_with("--- ") || line.starts_with("+++ ")
}

fn normalize_diff_path_line(line: &str, folder_a: &str, folder_b: &str) -> String {
    let mut out = line.to_string();
    for (prefix, folder) in [('a', folder_a), ('b', folder_b)] {
        let trimmed = folder.trim_matches('/');
        out = out.replace(&format!("{prefix}/{trimmed}/"), &format!("{prefix}/"));
        out = out.replace(&format!("{prefix}{folder}/"), &format!("{prefix}/"));
        out = out.replace(&format!("{folder}/"), "");
    }
    out
}

fn remove_ds_store_diff_blocks(diff: &str) -> String {
    let mut kept = String::new();
    let mut current = String::new();
    for line in diff.split_inclusive('\n') {
        if line.starts_with("diff --git ") {
            push_non_ds_store_block(&mut kept, &current);
            current.clear();
        }
        current.push_str(line);
    }
    push_non_ds_store_block(&mut kept, &current);
    kept
}

fn push_non_ds_store_block(output: &mut String, block: &str) {
    if block.is_empty() {
        return;
    }
    let header = block.lines().next().unwrap_or_default();
    if header.contains(".DS_Store") {
        return;
    }
    output.push_str(block);
}

#[cfg(test)]
mod tests;
