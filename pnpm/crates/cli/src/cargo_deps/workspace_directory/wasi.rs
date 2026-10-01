use super::{ManagedDirectory, Path, io};

pub(super) fn unsupported<T>() -> io::Result<T> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Cargo workspace dependency management requires pinned-directory filesystem operations, which this WebContainer host does not provide",
    ))
}

pub(in crate::cargo_deps) fn read_workspace_file(
    _directory: &ManagedDirectory,
    _name: &str,
) -> io::Result<(String, Option<u32>)> {
    unsupported()
}

pub(in crate::cargo_deps) fn write_workspace_file(
    _directory: &ManagedDirectory,
    _name: &str,
    _bytes: &[u8],
    _mode: Option<u32>,
) -> io::Result<()> {
    unsupported()
}

pub(in crate::cargo_deps) fn force_workspace_symlink(
    _directory: &ManagedDirectory,
    _target: &Path,
    _name: &str,
) -> io::Result<pnpm_fs::ForceSymlinkOutcome> {
    unsupported()
}
