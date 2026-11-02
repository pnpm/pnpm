use super::StoreFile;
use std::{cell::Cell, path::PathBuf, sync::Arc};

/// pnpm/pnpm#17543: the peer variants of a package share one files map, so
/// a mode the first import reads from disk answers every later import.
#[test]
fn a_mode_read_once_answers_every_import_sharing_the_file() {
    let file = Arc::new(StoreFile::new(PathBuf::from("files/00/blob")));
    let reads = Cell::new(0);
    let read = |_: &std::path::Path| {
        reads.set(reads.get() + 1);
        Ok(0o644)
    };

    let shared = Arc::clone(&file);
    assert_eq!(file.mode_or_read(read).unwrap(), 0o644);
    assert_eq!(shared.mode_or_read(read).unwrap(), 0o644);

    assert_eq!(reads.get(), 1);
    assert_eq!(shared.mode(), Some(0o644));
}

#[test]
fn a_known_mode_is_never_read_from_disk() {
    let file = StoreFile::with_mode(PathBuf::from("files/00/blob"), Some(0o755));
    let mode = file
        .mode_or_read(|_| panic!("the mode is known"))
        .unwrap();
    assert_eq!(mode, 0o755);
}

#[test]
fn equality_ignores_the_mode() {
    let path = PathBuf::from("files/00/blob");
    assert_eq!(StoreFile::new(path.clone()), StoreFile::with_mode(path, Some(0o644)));
}
