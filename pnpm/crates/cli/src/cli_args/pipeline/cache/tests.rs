use super::{RecordedFile, TaskCache, collect_output_files};
#[cfg(unix)]
use pnpm_crypto_hash::create_hex_hash_from_file;
use std::fs;

fn setup() -> (tempfile::TempDir, tempfile::TempDir, TaskCache) {
    let project = tempfile::tempdir().unwrap();
    let storage = tempfile::tempdir().unwrap();
    let cache = TaskCache::open(storage.path(), project.path()).unwrap();
    fs::create_dir(project.path().join("out")).unwrap();
    fs::write(project.path().join("out/result"), "built").unwrap();
    cache.store("abcdef", project.path(), "build", &["out/**".to_string()], Vec::new()).unwrap();
    (project, storage, cache)
}

#[test]
fn corrupted_outputs_are_a_miss_before_any_project_changes() {
    let (project, _storage, cache) = setup();
    let stored = cache.lookup("abcdef").unwrap();
    fs::write(stored.entry_dir.join("outputs/out/result"), "corrupt").unwrap();
    assert!(cache.restore(&stored, project.path(), "build").is_err());
    assert_eq!(fs::read_to_string(project.path().join("out/result")).unwrap(), "built");
}

#[test]
fn traversal_in_outputs_or_stale_records_cannot_escape() {
    let (project, _storage, cache) = setup();
    let mut stored = cache.lookup("abcdef").unwrap();
    for path in [
        "../outside",
        "/absolute",
        "out/../../outside",
        ".git/config",
        ".GIT/config",
        "node_modules/pkg",
        "Node_Modules/pkg",
    ] {
        stored.files = vec![path.to_string()];
        assert!(cache.restore(&stored, project.path(), "build").is_err(), "must reject {path}");
        stored.files.clear();
        cache
            .write_output_record(
                "build",
                &[RecordedFile { path: path.to_string(), hash: String::new() }],
            )
            .unwrap();
        assert!(
            cache.restore(&stored, project.path(), "build").is_err(),
            "must reject stale {path}",
        );
    }
}

#[cfg(unix)]
#[test]
fn symlinked_outputs_cannot_overwrite_or_delete_external_files() {
    let (project, _storage, cache) = setup();
    let outside = tempfile::tempdir().unwrap();
    let external = outside.path().join("result");
    fs::write(&external, "built").unwrap();
    fs::remove_dir_all(project.path().join("out")).unwrap();
    std::os::unix::fs::symlink(outside.path(), project.path().join("out")).unwrap();
    let mut stored = cache.lookup("abcdef").unwrap();
    assert!(cache.restore(&stored, project.path(), "build").is_err());
    stored.files.clear();
    cache
        .write_output_record(
            "build",
            &[RecordedFile {
                path: "out/result".to_string(),
                hash: create_hex_hash_from_file(&external).unwrap(),
            }],
        )
        .unwrap();
    assert!(cache.restore(&stored, project.path(), "build").is_err());
    assert_eq!(fs::read_to_string(external).unwrap(), "built");
}

#[test]
fn output_globs_select_only_declared_files_and_deduplicate() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("out")).unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    fs::write(project.path().join("out/result"), "built").unwrap();
    fs::write(project.path().join("src/main"), "source").unwrap();
    assert_eq!(
        collect_output_files(project.path(), &["out/**".to_string(), "out/result".to_string()])
            .unwrap(),
        ["out/result"],
    );
}

#[test]
fn repeat_publication_leaves_the_first_snapshot_complete() {
    let (project, _storage, cache) = setup();
    fs::write(project.path().join("out/result"), "changed").unwrap();
    let previous = fs::read(cache.output_record_path("build")).unwrap();
    fs::write(project.path().join("out/new-output"), "new output").unwrap();
    assert!(
        cache
            .store("abcdef", project.path(), "build", &["out/**".to_string()], Vec::new())
            .is_err(),
        "conflicting snapshots must not update restoration ownership",
    );
    assert_eq!(fs::read(cache.output_record_path("build")).unwrap(), previous);
    let stored = cache.lookup("abcdef").unwrap();
    assert_eq!(fs::read_to_string(stored.entry_dir.join("outputs/out/result")).unwrap(), "built");
    assert!(
        cache.restore(&stored, project.path(), "build").is_err(),
        "the changed working output must be preserved",
    );
    assert_eq!(fs::read_to_string(project.path().join("out/new-output")).unwrap(), "new output");
}

#[test]
fn concurrent_task_publications_leave_a_complete_snapshot() {
    let (project, _storage, cache) = setup();
    fs::remove_dir_all(cache.entry_dir("abcdef")).unwrap();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let publish = || {
            barrier.wait();
            cache
                .store("abcdef", project.path(), "build", &["out/**".to_string()], Vec::new())
                .unwrap();
        };
        let first = scope.spawn(publish);
        let second = scope.spawn(publish);
        first.join().unwrap();
        second.join().unwrap();
    });
    let stored = cache.lookup("abcdef").unwrap();
    cache.restore(&stored, project.path(), "build").unwrap();
    assert_eq!(fs::read_to_string(project.path().join("out/result")).unwrap(), "built");
}

#[test]
fn output_record_write_failures_are_reported() {
    let (project, _storage, cache) = setup();
    let record_path = cache.output_record_path("build");
    fs::remove_file(&record_path).unwrap();
    fs::create_dir(&record_path).unwrap();
    let stored = cache.lookup("abcdef").unwrap();
    assert!(cache.restore(&stored, project.path(), "build").is_err());
    assert!(
        cache
            .store("abcdef", project.path(), "build", &["out/**".to_string()], Vec::new())
            .is_err(),
    );
}

#[cfg(unix)]
#[test]
fn symlinked_project_roots_are_rejected() {
    let (project, storage, cache) = setup();
    let link = storage.path().join("linked-project");
    std::os::unix::fs::symlink(project.path(), &link).unwrap();
    let stored = cache.lookup("abcdef").unwrap();
    assert!(cache.restore(&stored, &link, "build").is_err());
    assert_eq!(fs::read_to_string(project.path().join("out/result")).unwrap(), "built");
}
