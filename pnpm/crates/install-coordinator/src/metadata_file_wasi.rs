use miette::{IntoDiagnostic, Result, WrapErr};
use std::{
    fs,
    io::{self, Read as _},
    path::{Path, PathBuf},
};

#[derive(PartialEq, Eq)]
enum FileState {
    Missing,
    Regular(Vec<u8>, pnpm_fs::CopyPermissions),
    Symlink(PathBuf),
}

pub(super) struct MetadataFile {
    path: PathBuf,
    state: FileState,
}

impl MetadataFile {
    pub(super) fn capture(path: PathBuf) -> Result<Self> {
        let path = pnpm_fs::lexical_normalize(&std::path::absolute(path).into_diagnostic()?);
        let state = read_state(&path)
            .into_diagnostic()
            .wrap_err_with(|| format!("snapshot {}", path.display()))?;
        Ok(Self { path, state })
    }

    pub(super) fn restore(self) -> Result<()> {
        if read_state(&self.path).into_diagnostic()? == self.state {
            return Ok(());
        }
        // Node exposes pathname operations but no descriptor-relative directory
        // operations. Restoring by pathname could write through a replaced parent.
        Err(miette::miette!(
            "Cannot safely restore {} in this WebContainer: the host does not support pinned-directory metadata rollback. Project metadata may contain changes from the failed operation.",
            self.path.display(),
        ))
    }
}

#[allow(clippy::verbose_file_reads, reason = "Read through the validated nofollow file descriptor")]
fn read_state(path: &Path) -> io::Result<FileState> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(FileState::Missing),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() {
        return fs::read_link(path).map(FileState::Symlink);
    }
    let mut file = pnpm_fs::open_file_without_following(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("project metadata path is not a regular file or symlink"));
    }
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)?;
    Ok(FileState::Regular(contents, pnpm_fs::read_file_permissions(&file)?))
}
