use super::{
    InputsUnavailable, RecordedFile, TaskCache, TaskKeyInputs, collect_output_files,
    fs_ops::replace_output_file,
};
use pnpm_config::TaskSettings;
#[cfg(unix)]
use pnpm_crypto_hash::{create_hex_hash_bytes, create_hex_hash_from_file};
use pnpm_lockfile::WantedLockfileSelection;
use pnpm_testing_utils::git_repo::GitRepoFixture;
use pnpm_workspace_task_scheduler::TaskNode;
use std::{collections::HashMap, fs, path::Path};

fn setup() -> (tempfile::TempDir, tempfile::TempDir, TaskCache) {
    let project = tempfile::tempdir().unwrap();
    let storage = tempfile::tempdir().unwrap();
    let cache = TaskCache::open(storage.path(), project.path()).unwrap();
    fs::create_dir(project.path().join("out")).unwrap();
    fs::write(project.path().join("out/result"), "built").unwrap();
    cache
        .store("abcdef", project.path(), "build", &["out/**".to_string()], Vec::new())
        .unwrap();
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
    let files = cache
        .hashed_project_files(&project)
        .unwrap()
        .unwrap();
    assert!(files.is_empty(), "deleted tracked input must be absent: {files:?}");
}

#[test]
fn hashing_inputs_reports_read_errors() {
    let (root, _repo, cache) = setup_input_cache();
    let project = root.path().join("inputs-src");
    fs::remove_file(project.join("input")).unwrap();
    fs::create_dir(project.join("input")).unwrap();
    let error = cache
        .hashed_project_files(&project)
        .unwrap_err()
        .to_string();
    assert!(error.contains("hashing cache input"), "{error}");
    assert!(error.contains("input") && error.contains(&project.display().to_string()), "{error}");
    assert!(
        cache.project_files
            .lock()
            .unwrap()
            .is_empty(),
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
    let error = cache
        .hashed_project_files(&project)
        .unwrap_err()
        .to_string();
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
    let files = cache
        .hashed_project_files(&project)
        .unwrap()
        .unwrap();
    for relative in ["src/input", r"src\input"] {
        let file = files
            .iter()
            .find(|file| file.rel_path == relative)
            .expect("each filename is retained");
        assert_eq!(file.hash, create_hex_hash_from_file(&project.join(relative)).unwrap());
    }
}

#[cfg(unix)]
#[test]
fn hashing_inputs_covers_a_dangling_symlink_by_its_target() {
    let (root, _repo, cache) = setup_input_cache();
    let project = root.path().join("inputs-src");
    std::os::unix::fs::symlink("missing", project.join("linked-input")).unwrap();
    let files = cache
        .hashed_project_files(&project)
        .unwrap()
        .unwrap();
    let file = files
        .iter()
        .find(|file| file.rel_path == "linked-input")
        .expect("linked input");
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
    let error = cache
        .hashed_project_files(&project)
        .unwrap_err()
        .to_string();
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
    let files = cache
        .hashed_project_files(&project)
        .unwrap()
        .unwrap();
    let file = files
        .iter()
        .find(|file| file.rel_path == "linked-input")
        .expect("linked input");
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
    let error = cache
        .hashed_project_files(&link)
        .unwrap_err()
        .to_string();
    assert!(error.contains("symlink") && error.contains("linked-project"), "{error}");
    assert!(
        cache.project_files
            .lock()
            .unwrap()
            .is_empty(),
        "unsafe inputs must not be cached",
    );
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
    let error = cache
        .hashed_project_files(&project)
        .unwrap_err()
        .to_string();
    assert!(error.contains("symlink") && error.contains("dir"), "{error}");
    assert!(
        cache.project_files
            .lock()
            .unwrap()
            .is_empty(),
        "unsafe inputs must not be cached",
    );
}

#[test]
fn hashing_inputs_reports_a_git_work_tree_as_available() {
    let (root, _repo, cache) = setup_input_cache();
    let project = root.path().join("inputs-src");
    assert!(
        cache
            .hashed_project_files(&project)
            .unwrap()
            .is_some(),
    );
    assert_eq!(cache.inputs_unavailable(&project), None);
}

#[test]
fn inputs_outside_a_work_tree_are_uncacheable() {
    let project = tempfile::tempdir().unwrap();
    let storage = tempfile::tempdir().unwrap();
    let cache = TaskCache::open(storage.path(), project.path()).unwrap();
    assert!(
        cache
            .hashed_project_files(project.path())
            .unwrap()
            .is_none(),
    );
    assert_eq!(cache.inputs_unavailable(project.path()), Some(InputsUnavailable::NoGit));
    assert!(
        !cache.project_files
            .lock()
            .unwrap()
            .is_empty(),
        "the verdict must be memoized, or every task of the project probes git again",
    );
}

#[test]
fn inputs_in_bare_git_repo_are_uncacheable() {
    let repo_dir = tempfile::tempdir().unwrap();
    let init = std::process::Command::new("git")
        .args(["init", "--bare"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert!(init.status.success());
    let bare_check = std::process::Command::new("git")
        .args(["rev-parse", "--is-bare-repository"])
        .current_dir(repo_dir.path())
        .output()
        .unwrap();
    assert!(bare_check.status.success() && bare_check.stdout.trim_ascii() == b"true");
    let storage = tempfile::tempdir().unwrap();
    let cache = TaskCache::open(storage.path(), repo_dir.path()).unwrap();
    assert!(
        cache
            .hashed_project_files(repo_dir.path())
            .unwrap()
            .is_none(),
    );
    assert_eq!(cache.inputs_unavailable(repo_dir.path()), Some(InputsUnavailable::NoGit));
}

fn make_readonly(path: &Path) {
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions).unwrap();
}

#[test]
fn restoring_cached_outputs_replaces_and_prunes_readonly_files_without_mutating_hardlinks() {
    let project = tempfile::tempdir().unwrap();
    let storage = tempfile::tempdir().unwrap();
    let cache = TaskCache::open(storage.path(), project.path()).unwrap();
    fs::create_dir(project.path().join("out")).unwrap();
    fs::write(project.path().join("out/result"), "first").unwrap();
    fs::write(project.path().join("out/stale"), "stale").unwrap();
    make_readonly(&project.path().join("out/result"));
    make_readonly(&project.path().join("out/stale"));
    cache
        .store("111111", project.path(), "build", &["out/**".to_string()], Vec::new())
        .unwrap();

    let hardlink_observer = storage.path().join("observer");
    fs::hard_link(project.path().join("out/result"), &hardlink_observer).unwrap();

    let alt_project = tempfile::tempdir().unwrap();
    fs::create_dir(alt_project.path().join("out")).unwrap();
    fs::write(alt_project.path().join("out/result"), "second").unwrap();
    make_readonly(&alt_project.path().join("out/result"));
    let alt_cache = TaskCache::open(storage.path(), alt_project.path()).unwrap();
    alt_cache
        .store("222222", alt_project.path(), "other", &["out/**".to_string()], Vec::new())
        .unwrap();

    let second = cache.lookup("222222").unwrap();
    cache.restore(&second, project.path(), "build").unwrap();
    assert_eq!(fs::read_to_string(project.path().join("out/result")).unwrap(), "second");
    assert_eq!(fs::read_to_string(&hardlink_observer).unwrap(), "first");
    let stale_exists = project
        .path()
        .join("out/stale")
        .exists();
    assert!(!stale_exists);

    let first = cache.lookup("111111").unwrap();
    cache.restore(&first, project.path(), "build").unwrap();
    assert_eq!(fs::read_to_string(project.path().join("out/result")).unwrap(), "first");
    assert_eq!(fs::read_to_string(project.path().join("out/stale")).unwrap(), "stale");
}

fn task_node_at(project: &Path) -> TaskNode {
    TaskNode {
        project: project.to_path_buf(),
        task_name: "build".to_string(),
        concurrency: None,
        scripts: vec!["build".to_string()],
        requested: true,
        dependencies: Vec::new(),
    }
}

fn key_for_inputs(
    workspace_root: &Path,
    project: &Path,
    inputs: Option<Vec<String>>,
    lockfile_dir: &Path,
) -> miette::Result<String> {
    key_for_inputs_with_selection(
        workspace_root,
        project,
        inputs,
        lockfile_dir,
        &WantedLockfileSelection::default(),
    )
}

fn key_for_inputs_with_selection(
    workspace_root: &Path,
    project: &Path,
    inputs: Option<Vec<String>>,
    lockfile_dir: &Path,
    lockfile_selection: &WantedLockfileSelection,
) -> miette::Result<String> {
    let storage = tempfile::tempdir().unwrap();
    let cache = TaskCache::open(storage.path(), workspace_root)?;
    let node = task_node_at(project);
    let mut settings = TaskSettings::default();
    settings.inputs = inputs;
    settings.outputs = Some(vec!["dist/**".to_string()]);
    let environment = HashMap::new();
    let script_bodies = vec![("build".to_string(), "echo build".to_string())];
    Ok(cache
        .compute_task_key(&TaskKeyInputs {
            node: &node,
            settings: Some(&settings),
            lockfile_dir,
            lockfile_selection,
            dependency_keys: &[],
            environment: &environment,
            script_bodies: &script_bodies,
        })?
        .expect("cache key is available"))
}

#[test]
fn empty_inputs_hashes_zero_files_while_omitted_inputs_hashes_tracked_files() {
    let (root, repo, _cache) = setup_input_cache();
    let project = root.path().join("inputs-src");
    let empty_before = key_for_inputs(&project, &project, Some(Vec::new()), &project).unwrap();
    let default_before = key_for_inputs(&project, &project, None, &project).unwrap();

    repo.write_file("input", "modified");
    repo.write_file("extra.txt", "extra");
    let empty_after = key_for_inputs(&project, &project, Some(Vec::new()), &project).unwrap();
    let default_after = key_for_inputs(&project, &project, None, &project).unwrap();

    assert_eq!(empty_before, empty_after);
    assert_ne!(default_before, default_after);
}

#[test]
fn additive_and_excluded_input_globs_cover_gitignored_and_sibling_files() {
    let root = tempfile::tempdir().unwrap();
    let repo = GitRepoFixture::init(root.path(), "ws");
    let workspace = root.path().join("ws-src");
    let project = workspace.join("packages/app");
    repo.write_file(".gitignore", ".env.local\ndist/\n");
    repo.write_file("packages/app/src/index.js", "v1");
    repo.write_file("packages/app/README.md", "docs-v1");
    repo.write_file("packages/shared/common.txt", "shared-v1");
    repo.write_file("packages/shared/ignored.bak", "bak-v1");
    let _ = repo.commit("workspace fixture");

    let env_only_spec = || Some(vec![String::from("+.env.local")]);
    let without_env = key_for_inputs(&workspace, &project, env_only_spec(), &workspace).unwrap();
    fs::write(project.join(".env.local"), "MODE=1").unwrap();
    let with_env_v1 = key_for_inputs(&workspace, &project, env_only_spec(), &workspace).unwrap();
    fs::write(project.join(".env.local"), "MODE=2").unwrap();
    let with_env_v2 = key_for_inputs(&workspace, &project, env_only_spec(), &workspace).unwrap();
    fs::remove_file(project.join(".env.local")).unwrap();
    let after_delete = key_for_inputs(&workspace, &project, env_only_spec(), &workspace).unwrap();
    assert_ne!(without_env, with_env_v1);
    assert_ne!(with_env_v1, with_env_v2);
    assert_eq!(without_env, after_delete);

    repo.write_file("packages/app/src/index.js", "v2");
    let after_tracked_edit =
        key_for_inputs(&workspace, &project, env_only_spec(), &workspace).unwrap();
    assert_ne!(without_env, after_tracked_edit);

    let combined = || {
        Some(vec![
            String::from("src/**"),
            String::from("+../shared/**"),
            String::from("!../shared/*.bak"),
        ])
    };
    let baseline = key_for_inputs(&workspace, &project, combined(), &workspace).unwrap();
    repo.write_file("packages/app/README.md", "docs-v2");
    repo.write_file("packages/shared/ignored.bak", "bak-v2");
    assert_eq!(baseline, key_for_inputs(&workspace, &project, combined(), &workspace).unwrap());
    repo.write_file("packages/shared/common.txt", "shared-v2");
    assert_ne!(baseline, key_for_inputs(&workspace, &project, combined(), &workspace).unwrap());

    let parent_excl = || Some(vec![String::from("+../**"), String::from("!../app/README.md")]);
    let parent_excl_base = key_for_inputs(&workspace, &project, parent_excl(), &workspace).unwrap();
    repo.write_file("packages/app/README.md", "docs-v3");
    assert_eq!(
        parent_excl_base,
        key_for_inputs(&workspace, &project, parent_excl(), &workspace).unwrap(),
    );
    repo.write_file("packages/app/src/index.js", "v3");
    assert_ne!(
        parent_excl_base,
        key_for_inputs(&workspace, &project, parent_excl(), &workspace).unwrap(),
    );
}

#[cfg(unix)]
#[test]
fn additive_input_globs_skip_fifos_without_blocking() {
    let (root, _repo, _cache) = setup_input_cache();
    let project = root.path().join("inputs-src");
    fs::create_dir(project.join(".runtime")).unwrap();
    fs::write(project.join(".runtime/config.json"), "v1").unwrap();
    let fifo_status = std::process::Command::new("mkfifo")
        .arg(project.join(".runtime/daemon.pipe"))
        .status()
        .unwrap();
    assert!(fifo_status.success());

    let spec = || Some(vec![String::from("+.runtime/**")]);
    let first = key_for_inputs(&project, &project, spec(), &project).unwrap();
    fs::write(project.join(".runtime/config.json"), "v2").unwrap();
    let second = key_for_inputs(&project, &project, spec(), &project).unwrap();
    assert_ne!(first, second);
}

#[test]
fn replace_output_file_keeps_existing_target_when_source_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("out.txt");
    fs::write(&target, "intact").unwrap();
    make_readonly(&target);
    assert!(replace_output_file(&dir.path().join("missing.txt"), &target).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "intact");
}

#[test]
fn additive_input_globs_reject_workspace_escape_and_invalid_paths() {
    let root = tempfile::tempdir().unwrap();
    let repo = GitRepoFixture::init(root.path(), "ws");
    let workspace = root.path().join("ws-src");
    let project = workspace.join("packages/app");
    repo.write_file("packages/app/src/index.js", "v1");
    fs::write(root.path().join("outside.txt"), "secret").unwrap();
    let _ = repo.commit("workspace fixture");

    for invalid in [
        "+../../../outside.txt",
        r"+..\..\..\outside.txt",
        "+/etc/passwd",
        r"+\etc\passwd",
        "+../.git/HEAD",
        "+../node_modules/a",
        r"+..\node_modules\a",
    ] {
        let result =
            key_for_inputs(&workspace, &project, Some(vec![invalid.to_string()]), &workspace);
        assert!(result.is_err(), "expected {invalid} to be rejected");
    }
    #[cfg(unix)]
    {
        let outside_dir = tempfile::tempdir().unwrap();
        fs::write(outside_dir.path().join("secret.txt"), "secret").unwrap();
        std::os::unix::fs::symlink(outside_dir.path(), project.join("linked")).unwrap();
        let result = key_for_inputs(
            &workspace,
            &project,
            Some(vec![String::from("+linked/**")]),
            &workspace,
        );
        assert!(result.is_err(), "symlinked additive directory must be rejected");
    }
    #[cfg(windows)]
    {
        let outside_dir = tempfile::tempdir().unwrap();
        fs::write(outside_dir.path().join("secret.txt"), "secret").unwrap();
        pnpm_fs::symlink_dir(outside_dir.path(), &project.join("linked")).unwrap();
        let result = key_for_inputs(
            &workspace,
            &project,
            Some(vec![String::from("+linked/**")]),
            &workspace,
        );
        assert!(result.is_err(), "junction additive directory must be rejected");
    }
}

#[test]
fn lockfile_selection_hashes_per_dir_and_branch_lockfile_fallbacks() {
    let (root, _repo, _cache) = setup_input_cache();
    let project = root.path().join("inputs-src");
    let first_dir = root.path().join("lock-a");
    let second_dir = root.path().join("lock-b");
    fs::create_dir_all(&first_dir).unwrap();
    fs::create_dir_all(&second_dir).unwrap();
    fs::write(first_dir.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\nimporters: {}\n").unwrap();
    fs::write(second_dir.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\nimporters: {}\n")
        .unwrap();

    let initial = key_for_inputs(&project, &project, None, &first_dir).unwrap();
    assert_eq!(initial, key_for_inputs(&project, &project, None, &second_dir).unwrap());

    fs::write(
        first_dir.join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\nsettings: { autoInstallPeers: false }\n",
    )
    .unwrap();
    assert_ne!(initial, key_for_inputs(&project, &project, None, &first_dir).unwrap());
    assert_eq!(initial, key_for_inputs(&project, &project, None, &second_dir).unwrap());

    let new_branch = WantedLockfileSelection {
        file_name: "pnpm-lock.feat.yaml".to_string(),
        merge_git_branch_lockfiles: false,
        branch_lockfile_candidates: Vec::new(),
    };
    let fallback_key = key_for_inputs_with_selection(
        &project,
        &project,
        Some(Vec::new()),
        &second_dir,
        &new_branch,
    )
    .unwrap();
    fs::write(
        second_dir.join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\nsettings: { autoInstallPeers: true }\n",
    )
    .unwrap();
    let after_shared_edit = key_for_inputs_with_selection(
        &project,
        &project,
        Some(Vec::new()),
        &second_dir,
        &new_branch,
    )
    .unwrap();
    assert_ne!(fallback_key, after_shared_edit);

    let detached_dir = root.path().join("lock-detached");
    fs::create_dir_all(&detached_dir).unwrap();
    fs::write(detached_dir.join("pnpm-lock.feat.yaml"), "lockfileVersion: '9.0'\nimporters: {}\n")
        .unwrap();
    let detached = WantedLockfileSelection {
        file_name: "pnpm-lock.yaml".to_string(),
        merge_git_branch_lockfiles: false,
        branch_lockfile_candidates: vec!["pnpm-lock.feat.yaml".to_string()],
    };
    let detached_before = key_for_inputs_with_selection(
        &project,
        &project,
        Some(Vec::new()),
        &detached_dir,
        &detached,
    )
    .unwrap();
    fs::write(
        detached_dir.join("pnpm-lock.feat.yaml"),
        "lockfileVersion: '9.0'\nsettings: { autoInstallPeers: false }\n",
    )
    .unwrap();
    let detached_after = key_for_inputs_with_selection(
        &project,
        &project,
        Some(Vec::new()),
        &detached_dir,
        &detached,
    )
    .unwrap();
    assert_ne!(detached_before, detached_after);

    let merge_mode = WantedLockfileSelection {
        file_name: "pnpm-lock.yaml".to_string(),
        merge_git_branch_lockfiles: true,
        branch_lockfile_candidates: Vec::new(),
    };
    fs::write(detached_dir.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\nimporters: {}\n")
        .unwrap();
    let merge_before = key_for_inputs_with_selection(
        &project,
        &project,
        Some(Vec::new()),
        &detached_dir,
        &merge_mode,
    )
    .unwrap();
    fs::write(
        detached_dir.join("pnpm-lock.feat.yaml"),
        "lockfileVersion: '9.0'\nsettings: { autoInstallPeers: true }\n",
    )
    .unwrap();
    let merge_after = key_for_inputs_with_selection(
        &project,
        &project,
        Some(Vec::new()),
        &detached_dir,
        &merge_mode,
    )
    .unwrap();
    assert_ne!(merge_before, merge_after);
}
