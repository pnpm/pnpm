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
        device: 0,
    };
    fs::write(&file.source, "unchanged").unwrap();
    file.import::<Unsupported>(
        &pnpm_deps_restorer::ImportState::new(),
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
            &pnpm_deps_restorer::ImportState::new(),
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
        device: 0,
    };
    fs::write(&file.source, "unchanged").unwrap();
    let error = file
        .import::<Missing>(
            &pnpm_deps_restorer::ImportState::new(),
            &std::sync::atomic::AtomicU8::new(0),
            PackageImportMethod::CloneOrCopy,
        )
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
        device: 0,
    };
    fs::write(&file.source, "source").unwrap();
    fs::write(&file.destination, "private").unwrap();
    let error = file
        .import::<Occupied>(
            &pnpm_deps_restorer::ImportState::new(),
            &std::sync::atomic::AtomicU8::new(0),
            PackageImportMethod::CloneOrCopy,
        )
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
        device: 0,
    };
    fs::write(&file.source, "unchanged").unwrap();
    let error = file
        .import::<Unsupported>(
            &pnpm_deps_restorer::ImportState::new(),
            &std::sync::atomic::AtomicU8::new(0),
            PackageImportMethod::Clone,
        )
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    assert!(!file.destination.exists());
}

struct Unlinkable;

impl FsReflink for Unlinkable {
    fn reflink(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

impl pnpm_deps_restorer::FsHardLink for Unlinkable {
    fn hard_link(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

struct Linkable;

impl FsReflink for Linkable {
    fn reflink(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

impl pnpm_deps_restorer::FsHardLink for Linkable {
    fn hard_link(source: &Path, destination: &Path) -> io::Result<()> {
        fs::copy(source, destination).map(|_| ())
    }
}

#[test]
fn fallback_in_one_filesystem_pair_does_not_disable_other_importers() {
    let temporary = tempfile::tempdir().unwrap();
    let source = temporary.path().join("source");
    fs::write(&source, "unchanged").unwrap();
    let state = pnpm_deps_restorer::ImportState::new();
    let logged = std::sync::atomic::AtomicU8::new(0);
    let method = state
        .import::<pnpm_reporter::SilentReporter, Unlinkable>(
            PackageImportMethod::Auto,
            &logged,
            &source,
            &temporary.path().join("unavailable"),
        )
        .unwrap();
    assert_eq!(method, pnpm_reporter::PackageImportMethod::Copy);
    let method = state
        .import::<pnpm_reporter::SilentReporter, Linkable>(
            PackageImportMethod::Auto,
            &logged,
            &source,
            &temporary.path().join("cached"),
        )
        .unwrap();
    assert_eq!(method, pnpm_reporter::PackageImportMethod::Copy);
    let fresh = pnpm_deps_restorer::ImportState::new();
    let method = fresh
        .import::<pnpm_reporter::SilentReporter, Linkable>(
            PackageImportMethod::Auto,
            &logged,
            &source,
            &temporary.path().join("available"),
        )
        .unwrap();
    assert_eq!(method, pnpm_reporter::PackageImportMethod::Hardlink);
}
