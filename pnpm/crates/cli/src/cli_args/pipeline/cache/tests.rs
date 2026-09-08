use super::{RecordedFile, TaskCache, collect_output_files};
#[cfg(unix)]
use pnpm_crypto_hash::{create_hex_hash_bytes, create_hex_hash_from_file};
use pnpm_testing_utils::git_repo::GitRepoFixture;
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

fn setup_input_cache() -> (tempfile::TempDir, GitRepoFixture, TaskCache) {
    let root = tempfile::tempdir().unwrap();
    let repo = GitRepoFixture::init(root.path(), "inputs");
    repo.write_file("input", "source");
    let _ = repo.commit("initial input");
    let cache =
        TaskCache::open(&root.path().join("cache"), &root.path().join("inputs-src")).unwrap();
    (root, repo, cache)
}

#[test]
fn hashing_inputs_preserves_deleted_tracked_files() {
    let (root, _repo, cache) = setup_input_cache();
    let project = root.path().join("inputs-src");
    fs::remove_file(project.join("input")).unwrap();
    let files = cache.hashed_project_files(&project).unwrap().unwrap();
    assert!(files.is_empty(), "deleted tracked input must be absent: {files:?}");
}

#[test]
fn hashing_inputs_reports_read_errors() {
    let (root, _repo, cache) = setup_input_cache();
    let project = root.path().join("inputs-src");
    fs::remove_file(project.join("input")).unwrap();
    fs::create_dir(project.join("input")).unwrap();
    let error = cache.hashed_project_files(&project).unwrap_err().to_string();
    assert!(error.contains("hashing cache input"), "{error}");
    assert!(error.contains("input") && error.contains(&project.display().to_string()), "{error}");
    assert!(
        cache.project_files.lock().unwrap().is_empty(),
        "failed enumeration must not be cached",
    );
}

#[test]
fn hashing_inputs_rejects_non_utf8_names() {
    let (root, _repo, cache) = setup_input_cache();
    let project = root.path().join("inputs-src");
    let blob = assert_cmd::Command::new("git")
        .current_dir(&project)
        .args(["rev-parse", "HEAD:input"])
        .assert()
        .success();
    let blob = String::from_utf8_lossy(&blob.get_output().stdout);
    let mut record = format!("100644 {}\tinvalid-", blob.trim()).into_bytes();
    record.extend_from_slice(b"\xff\0");
    assert_cmd::Command::new("git")
        .current_dir(&project)
        .args(["update-index", "-z", "--index-info"])
        .write_stdin(record)
        .assert()
        .success();
    let error = cache.hashed_project_files(&project).unwrap_err().to_string();
    assert!(error.contains("non-UTF-8") && error.contains("invalid-"), "{error}");
    assert!(error.contains(&project.display().to_string()), "{error}");
}

#[cfg(unix)]
#[test]
fn hashing_inputs_keeps_literal_backslashes_distinct_from_separators() {
    let (root, repo, cache) = setup_input_cache();
    let project = root.path().join("inputs-src");
    repo.write_file("src/input", "nested source");
    fs::write(project.join(r"src\input"), "literal source").unwrap();
    let files = cache.hashed_project_files(&project).unwrap().unwrap();
    for relative in ["src/input", r"src\input"] {
        let file =
            files.iter().find(|file| file.rel_path == relative).expect("each filename is retained");
        assert_eq!(file.hash, create_hex_hash_from_file(&project.join(relative)).unwrap());
    }
}

#[cfg(unix)]
#[test]
fn hashing_inputs_covers_a_dangling_symlink_by_its_target() {
    let (root, _repo, cache) = setup_input_cache();
    let project = root.path().join("inputs-src");
    std::os::unix::fs::symlink("missing", project.join("linked-input")).unwrap();
    let files = cache.hashed_project_files(&project).unwrap().unwrap();
    let file = files.iter().find(|file| file.rel_path == "linked-input").expect("linked input");
    assert_eq!(file.hash, format!("symlink:{}", create_hex_hash_bytes(b"missing")));
}

#[cfg(unix)]
#[test]
fn hashing_inputs_rejects_dangling_parent_symlinks() {
    let (root, repo, cache) = setup_input_cache();
    let project = root.path().join("inputs-src");
    repo.write_file("dir/input", "source");
    let _ = repo.commit("nested input");
    repo.write_file(".gitignore", "dir\n");
    fs::remove_file(project.join("dir/input")).unwrap();
    fs::remove_dir(project.join("dir")).unwrap();
    std::os::unix::fs::symlink("missing", project.join("dir")).unwrap();
    let error = cache.hashed_project_files(&project).unwrap_err().to_string();
    assert!(error.contains("symlink") && error.contains("dir"), "{error}");
}

#[cfg(unix)]
#[test]
fn hashing_inputs_covers_a_leaf_symlink_without_following_it() {
    let (root, _repo, cache) = setup_input_cache();
    let project = root.path().join("inputs-src");
    let outside = root.path().join("outside-input");
    fs::write(&outside, "external source").unwrap();
    std::os::unix::fs::symlink(&outside, project.join("linked-input")).unwrap();
    let files = cache.hashed_project_files(&project).unwrap().unwrap();
    let file = files.iter().find(|file| file.rel_path == "linked-input").expect("linked input");
    assert_eq!(
        file.hash,
        format!("symlink:{}", create_hex_hash_bytes(outside.as_os_str().as_encoded_bytes())),
    );
    assert_ne!(file.hash, create_hex_hash_from_file(&outside).unwrap());
}

#[cfg(unix)]
#[test]
fn hashing_inputs_rejects_symlinked_project_roots() {
    let (root, _repo, cache) = setup_input_cache();
    let link = root.path().join("linked-project");
    std::os::unix::fs::symlink(root.path().join("inputs-src"), &link).unwrap();
    let error = cache.hashed_project_files(&link).unwrap_err().to_string();
    assert!(error.contains("symlink") && error.contains("linked-project"), "{error}");
    assert!(cache.project_files.lock().unwrap().is_empty(), "unsafe inputs must not be cached");
}

#[cfg(unix)]
#[test]
fn hashing_inputs_rejects_valid_parent_symlinks() {
    let (root, repo, cache) = setup_input_cache();
    let project = root.path().join("inputs-src");
    repo.write_file("dir/input", "source");
    let _ = repo.commit("nested input");
    repo.write_file(".gitignore", "dir\n");
    fs::remove_file(project.join("dir/input")).unwrap();
    fs::remove_dir(project.join("dir")).unwrap();
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("input"), "external source").unwrap();
    std::os::unix::fs::symlink(&outside, project.join("dir")).unwrap();
    let error = cache.hashed_project_files(&project).unwrap_err().to_string();
    assert!(error.contains("symlink") && error.contains("dir"), "{error}");
    assert!(cache.project_files.lock().unwrap().is_empty(), "unsafe inputs must not be cached");
}
