use super::MetadataMutation;
use std::fs;

#[test]
fn restores_changed_and_new_files_after_an_error() {
    let directory = tempfile::tempdir().unwrap();
    let existing = directory.path().join("existing");
    let created = directory.path().join("created");
    fs::write(&existing, "before").unwrap();
    let mutation = MetadataMutation::capture([existing.clone(), created.clone()]).unwrap();

    fs::write(&existing, "after").unwrap();
    fs::write(&created, "new").unwrap();
    let error = mutation.finish(Err(miette::miette!("operation failed"))).unwrap_err();

    assert_eq!(error.to_string(), "operation failed");
    assert_eq!(fs::read_to_string(existing).unwrap(), "before");
    assert!(!created.exists());
}

#[test]
fn keeps_mutations_after_success() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("manifest");
    fs::write(&path, "before").unwrap();
    let mutation = MetadataMutation::capture([path.clone()]).unwrap();

    fs::write(&path, "after").unwrap();
    mutation.finish(Ok(())).unwrap();

    assert_eq!(fs::read_to_string(path).unwrap(), "after");
}

#[test]
fn attempts_every_restoration_after_one_fails() {
    let directory = tempfile::tempdir().unwrap();
    let existing = directory.path().join("a-existing");
    let unrestorable = directory.path().join("z-unrestorable");
    fs::write(&existing, "before").unwrap();
    let mutation = MetadataMutation::capture([existing.clone(), unrestorable.clone()]).unwrap();

    fs::write(&existing, "after").unwrap();
    fs::create_dir(&unrestorable).unwrap();
    let error = mutation.finish(Err(miette::miette!("operation failed"))).unwrap_err();

    assert!(error.to_string().contains("operation failed"));
    assert_eq!(fs::read_to_string(existing).unwrap(), "before");
}
