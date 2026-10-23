use miette::{IntoDiagnostic, Result, WrapErr};
#[cfg(unix)]
use std::io::Read as _;
use std::{
    ffi::{OsStr, OsString},
    fs, io,
    path::{Component, Path, PathBuf},
};

#[cfg(any(unix, windows))]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(any(unix, windows))]
static TEMPORARY_FILE_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, PartialEq, Eq)]
enum FileState {
    Missing,
    Regular {
        contents: Vec<u8>,
        #[cfg(unix)]
        mode: u32,
    },
    Symlink(PathBuf),
}

pub(super) struct MetadataFile {
    path: PathBuf,
    parent: PinnedDirectory,
    remaining_parent: Vec<OsString>,
    name: OsString,
    state: FileState,
}

impl MetadataFile {
    pub(super) fn capture(path: PathBuf) -> Result<Self> {
        let path = absolute_path(path)?;
        let path_display = path.display().to_string();
        let name = path
            .file_name()
            .ok_or_else(|| miette::miette!("metadata path has no file name: {path_display}"))?
            .to_os_string();
        let parent_path = path
            .parent()
            .ok_or_else(|| miette::miette!("metadata path has no parent: {path_display}"))?;
        let (parent, remaining_parent) = PinnedDirectory::nearest_existing(parent_path)
            .wrap_err_with(|| format!("pin parent of {}", path.display()))?;
        let state = read_from(&parent, &remaining_parent, &name)
            .into_diagnostic()
            .wrap_err_with(|| format!("snapshot {}", path.display()))?;
        Ok(Self { path, parent, remaining_parent, name, state })
    }

    pub(super) fn restore(self) -> Result<()> {
        let current = read_from(&self.parent, &self.remaining_parent, &self.name)
            .into_diagnostic()
            .wrap_err_with(|| format!("inspect {} before restoration", self.path.display()))?;
        if current == self.state {
            return Ok(());
        }
        let parent = self.parent
            .open_descendant(&self.remaining_parent)
            .into_diagnostic()
            .wrap_err_with(|| format!("open parent of {} for restoration", self.path.display()))?;
        let outcome = match self.state {
            FileState::Missing => remove_file(&parent, &self.name),
            FileState::Regular {
                contents,
                #[cfg(unix)]
                mode,
            } => write_file(
                &parent,
                &self.name,
                &contents,
                #[cfg(unix)]
                mode,
            ),
            FileState::Symlink(target) => write_symlink(&parent, &self.name, &target),
        };
        outcome
            .into_diagnostic()
            .wrap_err_with(|| format!("restore {}", self.path.display()))
    }
}

fn absolute_path(path: PathBuf) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().into_diagnostic()?.join(path)
    };
    let path_display = path.display().to_string();
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(miette::miette!("metadata path escapes its root: {path_display}"));
                }
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

fn read_from(
    base: &PinnedDirectory,
    parent_components: &[OsString],
    name: &OsStr,
) -> io::Result<FileState> {
    let parent = match base.open_descendant(parent_components) {
        Ok(parent) => parent,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(FileState::Missing),
        Err(error) => return Err(error),
    };
    read_file(&parent, name)
}

#[cfg(unix)]
fn read_file(parent: &PinnedDirectory, name: &OsStr) -> io::Result<FileState> {
    use std::os::unix::fs::PermissionsExt as _;

    match pnpm_ffi_fs::read_link_at(&parent.handle, name) {
        Ok(target) => return Ok(FileState::Symlink(target)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(FileState::Missing),
        // `InvalidInput` means the entry is not a symlink, which is the one
        // case that carries on to the open below.
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {}
        Err(error) => return Err(error),
    }
    // The non-blocking open is what lets a FIFO or a device at this name be
    // rejected below on its type: a blocking read-only open of one would
    // wait for a writer, and a metadata path never has one.
    let mut file = match pnpm_ffi_fs::open_file_at_without_blocking(&parent.handle, name) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(FileState::Missing),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::other("project metadata path is not a regular file or symlink"));
    }
    let mode = metadata.permissions().mode();
    let mut contents = Vec::new();
    #[expect(
        clippy::verbose_file_reads,
        reason = "the descriptor-relative open is what prevents a symlink race"
    )]
    file.read_to_end(&mut contents)?;
    Ok(FileState::Regular { contents, mode })
}

#[cfg(windows)]
fn read_file(parent: &PinnedDirectory, name: &OsStr) -> io::Result<FileState> {
    let path = parent.path.join(name);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(FileState::Missing),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() {
        return fs::read_link(path).map(FileState::Symlink);
    }
    if !metadata.is_file() || is_windows_reparse_point(&metadata) {
        return Err(io::Error::other("project metadata path is not a regular file or symlink"));
    }
    fs::read(path).map(|contents| FileState::Regular { contents })
}

#[cfg(unix)]
fn write_file(
    parent: &PinnedDirectory,
    name: &OsStr,
    contents: &[u8],
    mode: u32,
) -> io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt as _;

    let (temporary, mut file) = create_temporary_file(parent, name)?;
    let result = (|| {
        file.write_all(contents)?;
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        pnpm_fs::sync_file(&file)?;
        pnpm_ffi_fs::rename_at(&parent.handle, &temporary, name)
    })();
    drop(file);
    if result.is_err() {
        let _ = pnpm_ffi_fs::unlink_at(&parent.handle, &temporary);
    }
    result
}

#[cfg(windows)]
fn write_file(parent: &PinnedDirectory, name: &OsStr, contents: &[u8]) -> io::Result<()> {
    pnpm_fs::write_atomic(&parent.path.join(name), contents)
}

#[cfg(unix)]
fn write_symlink(parent: &PinnedDirectory, name: &OsStr, target: &Path) -> io::Result<()> {
    let temporary = temporary_name(name);
    pnpm_ffi_fs::symlink_at(target, &parent.handle, &temporary)?;
    let result = pnpm_ffi_fs::rename_at(&parent.handle, &temporary, name);
    if result.is_err() {
        let _ = pnpm_ffi_fs::unlink_at(&parent.handle, &temporary);
    }
    result
}

#[cfg(windows)]
fn write_symlink(parent: &PinnedDirectory, name: &OsStr, target: &Path) -> io::Result<()> {
    let destination = parent.path.join(name);
    let temporary = loop {
        let temporary = windows_temporary_path(parent, name);
        match std::os::windows::fs::symlink_file(target, &temporary) {
            Ok(()) => break temporary,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    };
    let result = replace_windows_path(&temporary, &destination);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
fn windows_temporary_path(parent: &PinnedDirectory, name: &OsStr) -> PathBuf {
    let mut temporary = OsString::from(".");
    temporary.push(name);
    temporary.push(format!(
        ".pnpm-{}-{}",
        pnpm_fs::process_id(),
        TEMPORARY_FILE_ID.fetch_add(1, Ordering::Relaxed),
    ));
    parent.path.join(temporary)
}

#[cfg(windows)]
fn replace_windows_path(source: &Path, destination: &Path) -> io::Result<()> {
    pnpm_ffi_fs::rename_over(source, destination)
}

#[cfg(unix)]
fn remove_file(parent: &PinnedDirectory, name: &OsStr) -> io::Result<()> {
    match pnpm_ffi_fs::unlink_at(&parent.handle, name) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

#[cfg(windows)]
fn remove_file(parent: &PinnedDirectory, name: &OsStr) -> io::Result<()> {
    match fs::remove_file(parent.path.join(name)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
/// Owner-only, so a staged metadata file is unreadable to anyone else even
/// where the umask would have allowed more.
const PRIVATE_FILE_MODE: pnpm_ffi_fs::FileMode = pnpm_ffi_fs::FileMode::from_mode(0o600);

fn create_temporary_file(
    parent: &PinnedDirectory,
    name: &OsStr,
) -> io::Result<(OsString, fs::File)> {
    loop {
        let temporary = temporary_name(name);
        match pnpm_ffi_fs::create_file_at(&parent.handle, &temporary, PRIVATE_FILE_MODE) {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}

#[cfg(unix)]
fn temporary_name(name: &OsStr) -> OsString {
    let mut temporary = OsString::from(".");
    temporary.push(name);
    temporary.push(format!(
        ".pnpm-{}-{}",
        pnpm_fs::process_id(),
        TEMPORARY_FILE_ID.fetch_add(1, Ordering::Relaxed),
    ));
    temporary
}

mod directory;
use directory::PinnedDirectory;
#[cfg(windows)]
use directory::is_windows_reparse_point;

#[cfg(all(test, unix))]
mod tests;
