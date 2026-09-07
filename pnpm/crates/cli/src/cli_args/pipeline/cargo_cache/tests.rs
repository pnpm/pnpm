use super::CargoCache;
use std::{fs, process::Command};

fn project() -> tempfile::TempDir {
    let project = tempfile::tempdir().unwrap();
    assert!(Command::new("git").arg("init").arg(project.path()).output().unwrap().status.success());
    fs::write(project.path().join(".gitignore"), "target/\n").unwrap();
    project
}

#[test]
fn restored_state_survives_eviction_and_is_independent() {
    let first_worktree = project();
    let second_worktree = project();
    let cache = tempfile::tempdir().unwrap();
    let entry = cache.path().join("entry");
    let publisher = CargoCache::open(first_worktree.path(), "target").unwrap();
    fs::create_dir_all(publisher.target.join("debug/incremental")).unwrap();
    let original = publisher.target.join("debug/incremental/state");
    fs::write(&original, b"original").unwrap();
    let modified = fs::metadata(&original).unwrap().modified().unwrap();
    publisher.publish(&entry, "inputs", &[]).unwrap();
    let consumer = CargoCache::open(second_worktree.path(), "target").unwrap();
    assert!(consumer.restore(&entry, "inputs").unwrap());
    let restored = consumer.target.join("debug/incremental/state");
    assert_eq!(fs::metadata(&restored).unwrap().modified().unwrap(), modified);
    fs::write(&restored, b"edited").unwrap();
    assert_eq!(fs::read(&original).unwrap(), b"original");
    assert_eq!(fs::read(entry.join("files/debug/incremental/state")).unwrap(), b"original");
    fs::remove_dir_all(cache.path()).unwrap();
    assert_eq!(fs::read(&restored).unwrap(), b"edited");
    consumer.prepare("inputs").unwrap();
}

#[test]
fn incomplete_and_corrupt_snapshots_never_expose_a_target() {
    for corrupt in [false, true] {
        let first_worktree = project();
        let second_worktree = project();
        let cache = tempfile::tempdir().unwrap();
        let entry = cache.path().join("entry");
        let publisher = CargoCache::open(first_worktree.path(), "target").unwrap();
        fs::create_dir(&publisher.target).unwrap();
        fs::write(publisher.target.join("state"), "good").unwrap();
        publisher.publish(&entry, "inputs", &[]).unwrap();
        if corrupt {
            fs::write(entry.join("files/state"), "bad").unwrap();
        } else {
            fs::remove_file(entry.join("files/state")).unwrap();
        }
        let consumer = CargoCache::open(second_worktree.path(), "target").unwrap();
        assert!(consumer.restore(&entry, "inputs").is_err());
        assert!(!consumer.target.exists());
    }
}

#[test]
fn existing_target_is_never_replaced_and_changed_inputs_invalidate_freshness() {
    let root = project();
    let cache = CargoCache::open(root.path(), "target").unwrap();
    fs::create_dir_all(cache.target.join("debug/.fingerprint")).unwrap();
    fs::create_dir_all(cache.target.join("debug/incremental")).unwrap();
    fs::write(cache.target.join("debug/.fingerprint/stale"), "stale").unwrap();
    fs::write(cache.target.join("debug/incremental/state"), "keep").unwrap();
    assert!(!cache.restore(&root.path().join("missing"), "inputs").unwrap());
    cache.prepare("changed").unwrap();
    assert!(!cache.target.join("debug/.fingerprint").exists());
    assert_eq!(fs::read_to_string(cache.target.join("debug/incremental/state")).unwrap(), "keep");
}

#[test]
fn target_paths_cannot_escape_or_replace_package_metadata() {
    let root = project();
    for path in ["", "../target", "/target", ".git/target", "node_modules/target", "."] {
        assert!(CargoCache::open(root.path(), path).is_err(), "accepted {path}");
    }
}

#[cfg(unix)]
#[test]
fn symlinks_are_rejected_for_targets_and_snapshots() {
    let root = project();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("target")).unwrap();
    assert!(CargoCache::open(root.path(), "target").is_err());
    fs::remove_file(root.path().join("target")).unwrap();
    let cache = CargoCache::open(root.path(), "target").unwrap();
    fs::create_dir(&cache.target).unwrap();
    std::os::unix::fs::symlink(outside.path(), cache.target.join("escape")).unwrap();
    assert!(cache.publish(&outside.path().join("snapshot"), "inputs", &[]).is_err());
}

#[test]
fn source_configuration_and_environment_changes_select_different_snapshots() {
    let root = project();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn value() -> u8 { 1 }").unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = 'cache-fixture'\nversion = '0.1.0'\n[workspace]\n",
    )
    .unwrap();
    assert!(
        Command::new("cargo")
            .current_dir(root.path())
            .args(["generate-lockfile", "--offline"])
            .output()
            .unwrap()
            .status
            .success(),
    );
    let cache = tempfile::tempdir().unwrap();
    let environment = std::collections::BTreeMap::new();
    let key = || super::snapshot_entry(cache.path(), root.path(), "task", &environment).unwrap().1;
    let original = key();
    fs::write(root.path().join("src/lib.rs"), "pub fn value() -> u8 { 2 }").unwrap();
    let changed_source = key();
    assert_ne!(original, changed_source);
    fs::create_dir(root.path().join(".cargo")).unwrap();
    fs::write(
        root.path().join(".cargo/config.toml"),
        "[build]\nrustflags = ['--cfg', 'custom_build']\n",
    )
    .unwrap();
    let changed_config = key();
    assert_ne!(changed_source, changed_config);
    let environment = std::collections::BTreeMap::from([(
        "RUSTFLAGS".to_string(),
        "--cfg another_build".to_string(),
    )]);
    let changed_environment =
        super::snapshot_entry(cache.path(), root.path(), "task", &environment).unwrap().1;
    assert_ne!(changed_config, changed_environment);
}

#[test]
fn incomplete_publication_is_not_visible_as_a_snapshot() {
    let root = project();
    let cache = tempfile::tempdir().unwrap();
    let entry = cache.path().join("entry");
    fs::create_dir(cache.path().join(".publish-interrupted")).unwrap();
    let consumer = CargoCache::open(root.path(), "target").unwrap();
    assert!(consumer.restore(&entry, "inputs").is_err());
    assert!(!consumer.target.exists());
}

#[test]
fn concurrent_publishers_leave_one_complete_immutable_snapshot() {
    let first_worktree = project();
    let second_worktree = project();
    let cache = tempfile::tempdir().unwrap();
    let entry = cache.path().join("entry");
    let publishers = [
        CargoCache::open(first_worktree.path(), "target").unwrap(),
        CargoCache::open(second_worktree.path(), "target").unwrap(),
    ];
    for (index, publisher) in publishers.iter().enumerate() {
        fs::create_dir(&publisher.target).unwrap();
        fs::write(publisher.target.join("state"), index.to_string()).unwrap();
    }
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        for publisher in &publishers {
            let barrier = &barrier;
            let entry = &entry;
            scope.spawn(move || {
                barrier.wait();
                publisher.publish(entry, "inputs", &[]).unwrap();
            });
        }
    });
    let winner = fs::read(entry.join("files/state")).unwrap();
    assert!(winner == b"0" || winner == b"1");
    for publisher in &publishers {
        fs::write(publisher.target.join("state"), "replacement").unwrap();
        publisher.publish(&entry, "inputs", &[]).unwrap();
    }
    assert_eq!(fs::read(entry.join("files/state")).unwrap(), winner);
    let consumer_root = project();
    let consumer = CargoCache::open(consumer_root.path(), "target").unwrap();
    assert!(consumer.restore(&entry, "inputs").unwrap());
    assert_eq!(fs::read(consumer.target.join("state")).unwrap(), winner);
}

#[test]
fn snapshot_storage_cannot_overlap_the_build_directory() {
    let root = project();
    let cache = CargoCache::open(root.path(), "target").unwrap();
    fs::create_dir(&cache.target).unwrap();
    fs::write(cache.target.join("state"), "keep").unwrap();
    for entry in [root.path().to_path_buf(), cache.target.clone(), cache.target.join("nested")] {
        assert!(cache.publish(&entry, "inputs", &[]).is_err());
        assert!(cache.restore(&entry, "inputs").is_err());
    }
    assert_eq!(fs::read_to_string(cache.target.join("state")).unwrap(), "keep");
}

#[cfg(target_os = "linux")]
#[test]
fn restoration_falls_back_to_copy_on_tmpfs() {
    let first_worktree = project();
    let second_worktree = tempfile::tempdir_in("/dev/shm").unwrap();
    assert!(
        Command::new("git")
            .arg("init")
            .arg(second_worktree.path())
            .output()
            .unwrap()
            .status
            .success(),
    );
    fs::write(second_worktree.path().join(".gitignore"), "target/\n").unwrap();
    let storage = tempfile::tempdir().unwrap();
    let entry = storage.path().join("entry");
    let publisher = CargoCache::open(first_worktree.path(), "target").unwrap();
    fs::create_dir(&publisher.target).unwrap();
    fs::write(publisher.target.join("state"), "original").unwrap();
    publisher.publish(&entry, "inputs", &[]).unwrap();
    assert!(
        reflink_copy::reflink(
            entry.join("files/state"),
            second_worktree.path().join("clone-probe")
        )
        .is_err(),
    );
    let consumer = CargoCache::open(second_worktree.path(), "target").unwrap();
    assert!(consumer.restore(&entry, "inputs").unwrap());
    fs::write(consumer.target.join("state"), "edited").unwrap();
    assert_eq!(fs::read_to_string(entry.join("files/state")).unwrap(), "original");
    fs::remove_dir_all(storage.path()).unwrap();
    assert_eq!(fs::read_to_string(consumer.target.join("state")).unwrap(), "edited");
}

#[test]
fn snapshots_preserve_read_only_files() {
    let publishing_project = project();
    let storage = tempfile::tempdir().unwrap();
    let cache = CargoCache::open(publishing_project.path(), "target").unwrap();
    fs::create_dir(&cache.target).unwrap();
    let source = cache.target.join("read-only");
    fs::write(&source, "immutable").unwrap();
    let mut permissions = fs::metadata(&source).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&source, permissions).unwrap();
    let timestamp = fs::metadata(&source).unwrap().modified().unwrap();
    let entry = storage.path().join("entry");
    cache.publish(&entry, "inputs", &[]).unwrap();
    let restored_project = project();
    let restored = CargoCache::open(restored_project.path(), "target").unwrap();
    assert!(restored.restore(&entry, "inputs").unwrap());
    let metadata = fs::metadata(restored.target.join("read-only")).unwrap();
    assert!(metadata.permissions().readonly());
    assert_eq!(metadata.modified().unwrap(), timestamp);
}
