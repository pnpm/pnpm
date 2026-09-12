use super::{
    LoadWorkspaceStateError, NodeLinker, ProjectEntry, UpdateWorkspaceStateError,
    WORKSPACE_STATE_FILENAME, WorkspaceState, WorkspaceStateSettings, get_file_path,
    load_workspace_state, now_millis, update_workspace_state,
};
use indexmap::IndexMap;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;
use tempfile::tempdir;

#[test]
fn file_path_matches_upstream() {
    let dir = std::path::Path::new("/tmp/example");
    assert_eq!(get_file_path(dir), dir.join("node_modules").join(".pnpm-workspace-state-v1.json"));
}

#[test]
fn write_and_load_round_trip() {
    let tmp = tempdir().expect("create temp dir");
    let workspace_dir = tmp.path();

    let mut projects = BTreeMap::new();
    projects.insert(
        workspace_dir.to_string_lossy().into_owned(),
        ProjectEntry { name: Some("my-pkg".into()), version: Some("1.2.3".into()) },
    );

    let mut patched = IndexMap::new();
    patched.insert("some-pkg".to_string(), "patches/some-pkg.patch".to_string());

    let state = WorkspaceState {
        last_validated_timestamp: now_millis(),
        projects,
        pnpmfiles: vec![],
        filtered_install: false,
        config_dependencies: None,
        settings: WorkspaceStateSettings {
            auto_install_peers: Some(true),
            dedupe_peer_dependents: Some(true),
            dev: Some(true),
            hoist_pattern: Some(vec!["*".into()]),
            hoist_workspace_packages: Some(true),
            node_linker: Some(NodeLinker::Isolated),
            optional: Some(true),
            patched_dependencies: Some(patched.clone()),
            production: Some(true),
            public_hoist_pattern: Some(vec![]),
            ..Default::default()
        },
    };

    update_workspace_state(workspace_dir, &state).expect("write state");

    let path = get_file_path(workspace_dir);
    assert!(path.is_file(), "state file should exist at {path:?}");

    let on_disk = std::fs::read_to_string(&path).expect("read state");
    assert!(on_disk.ends_with('\n'), "upstream appends a trailing newline");

    let loaded = load_workspace_state(workspace_dir).expect("load state").expect("file present");
    assert_eq!(loaded, state);
}

#[test]
fn load_returns_none_when_missing() {
    let tmp = tempdir().expect("create temp dir");
    let loaded = load_workspace_state(tmp.path()).expect("missing state is not an error");
    assert!(loaded.is_none());
}

#[test]
fn omits_settings_that_are_none() {
    let state = WorkspaceState {
        last_validated_timestamp: 0,
        projects: BTreeMap::new(),
        pnpmfiles: vec![],
        filtered_install: false,
        config_dependencies: None,
        settings: WorkspaceStateSettings { auto_install_peers: Some(true), ..Default::default() },
    };
    let serialized = serde_json::to_string(&state).expect("serialize");
    assert!(serialized.contains(r#""autoInstallPeers":true"#), "got: {serialized}");
    assert!(!serialized.contains("dedupePeerDependents"), "got: {serialized}");
    assert!(!serialized.contains("nodeLinker"), "got: {serialized}");
    assert!(!serialized.contains("configDependencies"), "got: {serialized}");
}

/// `packageExtensions` round-trips through `WorkspaceStateSettings`
/// as a JSON object preserving the wire shape (`<selector>:
/// { dependencies: { … } }`). The drift gate in
/// `optimistic_repeat_install::settings_match` reads this field, so
/// the on-disk shape has to stay stable for the gate to be byte-
/// comparable with pnpm's writer.
#[test]
fn package_extensions_round_trip() {
    let extensions = serde_json::json!({
        "bar": { "dependencies": { "baz": "2.0.0" } },
    });
    let state = WorkspaceState {
        last_validated_timestamp: 0,
        projects: BTreeMap::new(),
        pnpmfiles: vec![],
        filtered_install: false,
        config_dependencies: None,
        settings: WorkspaceStateSettings {
            package_extensions: Some(extensions.clone()),
            ..Default::default()
        },
    };

    let tmp = tempdir().expect("create temp dir");
    update_workspace_state(tmp.path(), &state).expect("write state");
    let loaded = load_workspace_state(tmp.path()).expect("load state").expect("file present");
    assert_eq!(loaded.settings.package_extensions.as_ref(), Some(&extensions));

    let on_disk = std::fs::read_to_string(get_file_path(tmp.path())).expect("read state");
    assert!(on_disk.contains(r#""packageExtensions""#), "got: {on_disk}");
}

#[test]
fn node_linker_serializes_lowercase() {
    let value = serde_json::to_value(NodeLinker::Isolated).expect("serialize");
    assert_eq!(value, serde_json::Value::from("isolated"));
    let value = serde_json::to_value(NodeLinker::Hoisted).expect("serialize");
    assert_eq!(value, serde_json::Value::from("hoisted"));
    let value = serde_json::to_value(NodeLinker::Pnp).expect("serialize");
    assert_eq!(value, serde_json::Value::from("pnp"));
}

#[test]
fn update_surfaces_create_dir_error_when_workspace_is_a_regular_file() {
    let tmp = tempdir().expect("create temp dir");
    let blocker = tmp.path().join("blocker");
    std::fs::write(&blocker, b"not a dir").expect("seed blocker file");

    let state = WorkspaceState {
        last_validated_timestamp: 0,
        projects: BTreeMap::new(),
        pnpmfiles: vec![],
        filtered_install: false,
        config_dependencies: None,
        settings: WorkspaceStateSettings::default(),
    };
    let err = update_workspace_state(&blocker, &state)
        .expect_err("create_dir_all on a regular-file ancestor should fail");
    assert!(
        matches!(err, UpdateWorkspaceStateError::CreateDir { .. }),
        "expected CreateDir error, got {err:?}",
    );
}

/// `load_workspace_state` surfaces non-NotFound read errors via
/// the typed `ReadFile` variant. Here we make the target a
/// directory: `read_to_string` on a directory returns
/// `IsADirectory` (or `Other`) on Unix, never `NotFound`, which
/// keeps the early-return arm out of the way.
#[cfg(unix)]
#[test]
fn load_surfaces_read_file_error_when_target_is_a_directory() {
    let tmp = tempdir().expect("create temp dir");
    let workspace_dir = tmp.path();
    let target = get_file_path(workspace_dir);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::create_dir(&target).expect("seed directory at state path");

    let err =
        load_workspace_state(workspace_dir).expect_err("read_to_string on a directory should fail");
    assert!(
        matches!(err, LoadWorkspaceStateError::ReadFile { .. }),
        "expected ReadFile error, got {err:?}",
    );
}

#[test]
fn load_surfaces_parse_json_error_on_malformed_state() {
    let tmp = tempdir().expect("create temp dir");
    let workspace_dir = tmp.path();
    let target = get_file_path(workspace_dir);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, b"{ not valid json").expect("seed malformed state");

    let err = load_workspace_state(workspace_dir).expect_err("malformed JSON should fail to parse");
    assert!(
        matches!(err, LoadWorkspaceStateError::ParseJson { .. }),
        "expected ParseJson error, got {err:?}",
    );
}

/// The write-path tests below assert where the bytes land, not what is
/// in them, so the payload stays empty.
fn empty_state() -> WorkspaceState {
    WorkspaceState {
        last_validated_timestamp: 0,
        projects: BTreeMap::new(),
        pnpmfiles: vec![],
        filtered_install: false,
        config_dependencies: None,
        settings: WorkspaceStateSettings::default(),
    }
}

fn node_modules_entries(workspace_dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(workspace_dir.join("node_modules"))
        .expect("read node_modules")
        .map(|entry| entry.expect("read entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The temp file the write goes through is disarmed before the
/// rename, so nothing deletes it on drop any more. A successful
/// rename has to be what removes it from the directory.
#[test]
fn write_leaves_no_temp_file_beside_the_state_file() {
    let tmp = tempdir().expect("create temp dir");
    let workspace_dir = tmp.path();

    update_workspace_state(workspace_dir, &empty_state()).expect("write state");
    update_workspace_state(workspace_dir, &empty_state()).expect("overwrite state");

    assert_eq!(node_modules_entries(workspace_dir), vec![WORKSPACE_STATE_FILENAME.to_string()]);
}

/// A rename that cannot succeed still has to clean up after the
/// disarmed temp file. Renaming onto a directory fails outright on
/// Unix (`IsADirectory` / `NotADirectory`), which the retry
/// classifier leaves alone; on Windows the same setup reports
/// `ERROR_ACCESS_DENIED`, which *is* retried, so this stays Unix-only
/// rather than burning the retry budget.
#[cfg(unix)]
#[test]
fn failed_rename_reports_write_error_and_leaves_no_temp_file() {
    let tmp = tempdir().expect("create temp dir");
    let workspace_dir = tmp.path();
    let target = get_file_path(workspace_dir);
    std::fs::create_dir_all(&target).expect("seed a directory where the state file goes");

    let err = update_workspace_state(workspace_dir, &empty_state())
        .expect_err("renaming onto a directory should fail");
    assert!(
        matches!(err, UpdateWorkspaceStateError::WriteFile { .. }),
        "expected WriteFile error, got {err:?}",
    );
    assert_eq!(node_modules_entries(workspace_dir), vec![WORKSPACE_STATE_FILENAME.to_string()]);
}

/// The write survives another process holding the destination open
/// without `FILE_SHARE_DELETE` — an antivirus scan or the search
/// indexer — for as long as that handle lives
/// ([#14550](https://github.com/pnpm/pnpm/issues/14550)). A single
/// `MoveFileEx` fails such a rename with `ERROR_ACCESS_DENIED`, so
/// without the retry this write reports
/// `ERR_PNPM_WORKSPACE_STATE_WRITE_IO` instead of waiting the lock
/// out. Windows-only: on Unix the rename is not blocked by an open
/// handle at all, so there is nothing to retry.
#[cfg(windows)]
#[test]
fn transient_lock_on_the_state_file_does_not_fail_the_write() {
    use std::os::windows::fs::OpenOptionsExt as _;

    /// `FILE_SHARE_READ | FILE_SHARE_WRITE`, deliberately without
    /// `FILE_SHARE_DELETE`: that is the share mode that blocks a
    /// rename over the open file.
    const SHARE_READ_WRITE: u32 = 0x0000_0001 | 0x0000_0002;
    const HOLD: std::time::Duration = std::time::Duration::from_millis(300);

    let tmp = tempdir().expect("create temp dir");
    let workspace_dir = tmp.path();
    update_workspace_state(workspace_dir, &empty_state()).expect("seed state file");

    let handle = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ_WRITE)
        .open(get_file_path(workspace_dir))
        .expect("hold the state file open");
    // Start the clock before the releaser, so the measured wait always
    // covers the whole hold: taking it afterwards lets a descheduled main
    // thread miss part of the countdown and fail a retry that worked.
    let started = std::time::Instant::now();
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(HOLD);
        drop(handle);
    });

    update_workspace_state(workspace_dir, &empty_state())
        .expect("the write should wait the transient lock out");
    let waited = started.elapsed();

    releaser.join().expect("release the handle");
    // Succeeding is not on its own evidence that the retry did it: a rename
    // the handle never blocked would have returned immediately and passed
    // just the same. Only having waited for the handle shows otherwise.
    assert!(
        waited >= HOLD / 2,
        "the rename returned after {waited:?}, so the open handle never blocked it",
    );
    assert_eq!(node_modules_entries(workspace_dir), vec![WORKSPACE_STATE_FILENAME.to_string()]);
}
