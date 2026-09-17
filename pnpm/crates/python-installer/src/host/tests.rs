use super::FileImport;
use pnpm_config::PythonLinkMode;
use pnpm_fs::FsReflink;
use std::{fs, io, path::Path};

#[cfg(unix)]
mod unix;

struct PermissionDenied;

impl FsReflink for PermissionDenied {
    fn reflink(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::ErrorKind::PermissionDenied.into())
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
fn denied_reflinks_fall_back_to_private_copies_and_report_copy_errors() {
    let temporary = tempfile::tempdir().unwrap();
    let file = FileImport {
        source: temporary.path().join("source"),
        destination: temporary.path().join("destination"),
        executable: false,
        device: 0,
    };
    fs::write(&file.source, "unchanged").unwrap();
    assert!(file.import::<PermissionDenied>(PythonLinkMode::Reflink).unwrap());
    assert_eq!(fs::read_to_string(&file.destination).unwrap(), "unchanged");
    fs::write(&file.destination, "changed").unwrap();
    assert_eq!(fs::read_to_string(&file.source).unwrap(), "unchanged");
    fs::remove_file(&file.source).unwrap();
    let error = file.import::<PermissionDenied>(PythonLinkMode::Reflink).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

#[test]
fn missing_reflink_sources_do_not_trigger_copy_fallback() {
    let temporary = tempfile::tempdir().unwrap();
    let file = FileImport {
        source: temporary.path().join("source"),
        destination: temporary.path().join("destination"),
        executable: false,
        device: 0,
    };
    fs::write(&file.source, "unchanged").unwrap();
    let error = file.import::<Missing>(PythonLinkMode::Reflink).unwrap_err();
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
        device: 0,
    };
    fs::write(&file.source, "source").unwrap();
    fs::write(&file.destination, "private").unwrap();
    let error = file.import::<Occupied>(PythonLinkMode::Reflink).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read_to_string(&file.destination).unwrap(), "private");
}
