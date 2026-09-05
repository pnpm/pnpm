use super::MetadataMutation;
use std::fs;

async fn capture(
    directory: &tempfile::TempDir,
    paths: impl IntoIterator<Item = std::path::PathBuf>,
) -> MetadataMutation {
    MetadataMutation::capture(directory.path().join("locks"), directory.path().to_path_buf(), paths)
        .await
        .unwrap()
}

#[tokio::test]
async fn restores_changed_and_new_files_after_an_error() {
    let directory = tempfile::tempdir().unwrap();
    let existing = directory.path().join("existing");
    let created = directory.path().join("created");
    fs::write(&existing, "before").unwrap();
    let mutation = capture(&directory, [existing.clone(), created.clone()]).await;

    fs::write(&existing, "after").unwrap();
    fs::write(&created, "new").unwrap();
    let error = mutation.finish(Err(miette::miette!("operation failed"))).unwrap_err();

    assert_eq!(error.to_string(), "operation failed");
    assert_eq!(fs::read_to_string(existing).unwrap(), "before");
    eprintln!("created metadata path after rollback: {created:?}");
    assert!(!created.exists());
}

#[tokio::test]
async fn keeps_mutations_after_success() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("manifest");
    fs::write(&path, "before").unwrap();
    let mutation = capture(&directory, [path.clone()]).await;

    fs::write(&path, "after").unwrap();
    mutation.finish(Ok(())).unwrap();

    assert_eq!(fs::read_to_string(path).unwrap(), "after");
}

#[tokio::test]
async fn attempts_every_restoration_after_one_fails() {
    let directory = tempfile::tempdir().unwrap();
    let existing = directory.path().join("a-existing");
    let unrestorable = directory.path().join("z-unrestorable");
    fs::write(&existing, "before").unwrap();
    let mutation = capture(&directory, [existing.clone(), unrestorable.clone()]).await;

    fs::write(&existing, "after").unwrap();
    fs::create_dir(&unrestorable).unwrap();
    let error = mutation.finish(Err(miette::miette!("operation failed"))).unwrap_err();

    eprintln!("restoration failure: {error:?}");
    assert!(error.to_string().contains("operation failed"));
    assert_eq!(fs::read_to_string(existing).unwrap(), "before");
}

#[tokio::test]
async fn serializes_metadata_transactions_for_the_same_workspace() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("manifest");
    fs::write(&path, "before").unwrap();
    let first = capture(&directory, [path.clone()]).await;
    let lock_directory = directory.path().join("locks");
    let transaction_key = directory.path().to_path_buf();
    let second = tokio::spawn(async move {
        MetadataMutation::capture(lock_directory, transaction_key, [path]).await
    });

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    eprintln!(
        "second metadata transaction finished while the first held its lock: {}",
        second.is_finished(),
    );
    assert!(!second.is_finished());

    drop(first);
    tokio::time::timeout(std::time::Duration::from_secs(1), second)
        .await
        .expect("second transaction should acquire the released lock")
        .unwrap()
        .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn restores_a_metadata_symlink_without_replacing_it_with_a_file() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target");
    let path = directory.path().join("manifest");
    fs::write(&target, "before").unwrap();
    symlink("target", &path).unwrap();
    let mutation = capture(&directory, [path.clone()]).await;

    fs::remove_file(&path).unwrap();
    fs::write(&path, "after").unwrap();
    mutation.finish(Err(miette::miette!("operation failed"))).unwrap_err();

    assert_eq!(fs::read_link(&path).unwrap(), std::path::Path::new("target"));
    assert_eq!(fs::read_to_string(target).unwrap(), "before");
}

#[cfg(unix)]
#[tokio::test]
async fn restoration_stays_in_the_parent_pinned_during_capture() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("project");
    let original = directory.path().join("original-project");
    let attacker = directory.path().join("attacker");
    fs::create_dir(&project).unwrap();
    fs::create_dir(&attacker).unwrap();
    let path = project.join("manifest");
    fs::write(&path, "before").unwrap();
    fs::write(attacker.join("manifest"), "attacker").unwrap();
    let mutation = capture(&directory, [path]).await;

    fs::rename(&project, &original).unwrap();
    symlink(&attacker, &project).unwrap();
    fs::write(original.join("manifest"), "after").unwrap();
    mutation.finish(Err(miette::miette!("operation failed"))).unwrap_err();

    assert_eq!(fs::read_to_string(original.join("manifest")).unwrap(), "before");
    assert_eq!(fs::read_to_string(attacker.join("manifest")).unwrap(), "attacker");
}

#[cfg(unix)]
#[tokio::test]
async fn restoration_rejects_a_new_symlink_in_a_missing_parent_path() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("project");
    let attacker = directory.path().join("attacker");
    fs::create_dir(&project).unwrap();
    fs::create_dir(&attacker).unwrap();
    fs::write(attacker.join("config.toml"), "attacker").unwrap();
    let path = project.join(".cargo/config.toml");
    let mutation = capture(&directory, [path]).await;

    symlink(&attacker, project.join(".cargo")).unwrap();
    let error = mutation.finish(Err(miette::miette!("operation failed"))).unwrap_err();

    assert!(
        error.to_string().contains("operation failed"),
        "rollback should retain the operation error: {error:?}",
    );
    assert_eq!(fs::read_to_string(attacker.join("config.toml")).unwrap(), "attacker");
}
