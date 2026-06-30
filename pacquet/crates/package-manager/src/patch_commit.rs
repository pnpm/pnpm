use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_git_fetcher::PacklistError;
use pacquet_package_manifest::PackageManifestError;
use serde_json::{Map, Value};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_DIFF_OUTPUT_BYTES: u64 = 128 * 1024 * 1024;

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

    #[display("Unsafe temporary patch directory {}: {reason}", dir.display())]
    #[diagnostic(code(pacquet_package_manager::patch_commit_unsafe_temp_dir))]
    UnsafeTempDir { dir: PathBuf, reason: &'static str },

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

    #[display("git diff {stream} output exceeded {limit} bytes")]
    #[diagnostic(code(pacquet_package_manager::patch_commit_diff_output_too_large))]
    DiffOutputTooLarge { stream: &'static str, limit: u64 },
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
    let stdout = DiffTempFile::new("stdout")?;
    let stderr = DiffTempFile::new("stderr")?;
    let status = Command::new("git")
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
        .arg("--")
        .arg(&folder_a_slash)
        .arg(&folder_b_slash)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdout(stdout.writer.try_clone().map_err(|source| PatchCommitError::DiffSpawn { source })?)
        .stderr(stderr.writer.try_clone().map_err(|source| PatchCommitError::DiffSpawn { source })?)
        .status()
        .map_err(|source| PatchCommitError::DiffSpawn { source })?;

    let stderr = stderr.read_to_string("stderr")?;
    if !stderr.is_empty() || !matches!(status.code(), Some(0 | 1)) {
        return Err(PatchCommitError::DiffFailed { stderr });
    }
    let stdout = stdout.read_to_string("stdout")?;
    Ok(normalize_diff_output(&stdout, &folder_a_slash, &folder_b_slash))
}

fn remove_existing_temp_dir_with_fs(
    temp_dir: &Path,
    fs_ops: &impl PatchCommitFs,
) -> Result<(), PatchCommitError> {
    let metadata = match fs_ops.symlink_metadata(temp_dir) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(PatchCommitError::RemoveTempDir { dir: temp_dir.to_path_buf(), source });
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(PatchCommitError::UnsafeTempDir {
            dir: temp_dir.to_path_buf(),
            reason: "must not be a symbolic link",
        });
    }
    match fs_ops.remove_dir_all(temp_dir) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(PatchCommitError::RemoveTempDir { dir: temp_dir.to_path_buf(), source }),
    }
}

struct DiffTempFile {
    path: PathBuf,
    writer: File,
}

impl DiffTempFile {
    fn new(stream: &'static str) -> Result<Self, PatchCommitError> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let pid = std::process::id();
        let temp_dir = std::env::temp_dir();
        for _ in 0..16 {
            let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = temp_dir.join(format!("pacquet-git-diff-{stream}-{pid}-{counter}.tmp"));
            match diff_temp_file_options().open(&path) {
                Ok(writer) => return Ok(Self { path, writer }),
                Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
                Err(source) => return Err(PatchCommitError::DiffSpawn { source }),
            }
        }
        Err(PatchCommitError::DiffSpawn {
            source: io::Error::new(
                io::ErrorKind::AlreadyExists,
                "exhausted temp-path attempts for git diff output",
            ),
        })
    }

    fn read_to_string(&self, stream: &'static str) -> Result<String, PatchCommitError> {
        let len = fs::metadata(&self.path)
            .map_err(|source| PatchCommitError::DiffSpawn { source })?
            .len();
        if len > MAX_DIFF_OUTPUT_BYTES {
            return Err(PatchCommitError::DiffOutputTooLarge {
                stream,
                limit: MAX_DIFF_OUTPUT_BYTES,
            });
        }
        let mut file =
            File::open(&self.path).map_err(|source| PatchCommitError::DiffSpawn { source })?;
        let mut bytes = Vec::with_capacity(len as usize);
        let mut buffer = [0; 8192];
        loop {
            let read =
                file.read(&mut buffer).map_err(|source| PatchCommitError::DiffSpawn { source })?;
            if read == 0 {
                break;
            }
            let next_len =
                bytes.len().checked_add(read).ok_or(PatchCommitError::DiffOutputTooLarge {
                    stream,
                    limit: MAX_DIFF_OUTPUT_BYTES,
                })?;
            if next_len as u64 > MAX_DIFF_OUTPUT_BYTES {
                return Err(PatchCommitError::DiffOutputTooLarge {
                    stream,
                    limit: MAX_DIFF_OUTPUT_BYTES,
                });
            }
            bytes.extend_from_slice(&buffer[..read]);
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

fn diff_temp_file_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    options
}

impl Drop for DiffTempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

trait PatchCommitFs {
    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata>;
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    fn hard_link(&self, source: &Path, target: &Path) -> io::Result<()>;
    fn remove_dir_all(&self, path: &Path) -> io::Result<()>;
}

struct RealPatchCommitFs;

impl PatchCommitFs for RealPatchCommitFs {
    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
        fs::symlink_metadata(path)
    }

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
    if is_ds_store_diff_header(header) {
        return;
    }
    output.push_str(block);
}

fn is_ds_store_diff_header(header: &str) -> bool {
    let mut parts = header.split_whitespace();
    matches!(parts.next(), Some("diff"))
        && matches!(parts.next(), Some("--git"))
        && parts.take(2).all(|path| path.rsplit('/').next() == Some(".DS_Store"))
}

#[cfg(test)]
mod tests;
