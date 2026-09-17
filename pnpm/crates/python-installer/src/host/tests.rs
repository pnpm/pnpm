use super::FileImport;
use pnpm_config::PackageImportMethod;
use pnpm_fs::FsReflink;
use std::{fs, io, path::Path};

#[cfg(unix)]
mod unix;

struct Unsupported;

impl FsReflink for Unsupported {
    fn reflink(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

struct Missing;

impl FsReflink for Missing {
    fn reflink(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::ErrorKind::NotFound.into())
    }
}

struct Occupied;

impl FsReflink for Occupied {
    fn reflink(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::ErrorKind::AlreadyExists.into())
    }
}

#[test]
fn unsupported_reflinks_fall_back_to_private_copies_and_report_copy_errors() {
    let temporary = tempfile::tempdir().unwrap();
    let file = FileImport {
        source: temporary.path().join("source"),
        destination: temporary.path().join("destination"),
        executable: false,
    };
    fs::write(&file.source, "unchanged").unwrap();
    file.import::<Unsupported>(
        &std::sync::atomic::AtomicU8::new(0),
        PackageImportMethod::CloneOrCopy,
    )
    .unwrap();
    assert_eq!(fs::read_to_string(&file.destination).unwrap(), "unchanged");
    fs::write(&file.destination, "changed").unwrap();
    assert_eq!(fs::read_to_string(&file.source).unwrap(), "unchanged");
    fs::remove_file(&file.source).unwrap();
    let error = file
        .import::<Unsupported>(
            &std::sync::atomic::AtomicU8::new(0),
            PackageImportMethod::CloneOrCopy,
        )
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

#[test]
fn missing_reflink_sources_do_not_trigger_copy_fallback() {
    let temporary = tempfile::tempdir().unwrap();
    let file = FileImport {
        source: temporary.path().join("source"),
        destination: temporary.path().join("destination"),
        executable: false,
    };
    fs::write(&file.source, "unchanged").unwrap();
    let error = file
        .import::<Missing>(&std::sync::atomic::AtomicU8::new(0), PackageImportMethod::CloneOrCopy)
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(!file.destination.exists());
}

#[test]
fn existing_reflink_targets_are_not_overwritten_by_copy_fallback() {
    let temporary = tempfile::tempdir().unwrap();
    let file = FileImport {
        source: temporary.path().join("source"),
        destination: temporary.path().join("destination"),
        executable: false,
    };
    fs::write(&file.source, "source").unwrap();
    fs::write(&file.destination, "private").unwrap();
    let error = file
        .import::<Occupied>(&std::sync::atomic::AtomicU8::new(0), PackageImportMethod::CloneOrCopy)
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read_to_string(&file.destination).unwrap(), "private");
}

impl pnpm_deps_restorer::FsHardLink for Unsupported {
    fn hard_link(source: &Path, destination: &Path) -> io::Result<()> {
        fs::hard_link(source, destination)
    }
}

impl pnpm_deps_restorer::FsHardLink for Missing {
    fn hard_link(source: &Path, destination: &Path) -> io::Result<()> {
        fs::hard_link(source, destination)
    }
}

impl pnpm_deps_restorer::FsHardLink for Occupied {
    fn hard_link(source: &Path, destination: &Path) -> io::Result<()> {
        fs::hard_link(source, destination)
    }
}

#[test]
fn explicit_clone_does_not_fall_back_to_copy() {
    let temporary = tempfile::tempdir().unwrap();
    let file = FileImport {
        source: temporary.path().join("source"),
        destination: temporary.path().join("destination"),
        executable: false,
    };
    fs::write(&file.source, "unchanged").unwrap();
    let error = file
        .import::<Unsupported>(&std::sync::atomic::AtomicU8::new(0), PackageImportMethod::Clone)
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    assert!(!file.destination.exists());
}
