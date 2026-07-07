use super::{
    EditDirState, StateFileError, edit_dir_key, read_edit_dir_state, write_edit_dir_state,
    write_state_file_atomically,
};
use pacquet_config::{GetCurrentDir, Host};
use pretty_assertions::assert_eq;
use serde_json::json;
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use tempfile::tempdir;

fn sample_state() -> EditDirState {
    EditDirState {
        patched_pkg: "is-positive@1.0.0".to_string(),
        apply_to_all: false,
        package_key: None,
    }
}

#[test]
fn patch_state_read_missing_state_file_returns_none() {
    let tmp = tempdir().expect("temp dir");
    let modules_dir = tmp.path().join("node_modules");
    let edit_dir = tmp.path().join("edit");

    assert_eq!(read_edit_dir_state::<Host>(&modules_dir, &edit_dir).unwrap(), None);
}

#[test]
fn patch_state_write_creates_pnpm_state_file() {
    let tmp = tempdir().expect("temp dir");
    let modules_dir = tmp.path().join("node_modules");
    let edit_dir = tmp.path().join("edit");
    fs::create_dir_all(&edit_dir).expect("create edit dir");

    write_edit_dir_state::<Host>(&modules_dir, &edit_dir, &sample_state()).unwrap();

    let state_path = modules_dir.join(".pnpm_patches").join("state.json");
    let text = fs::read_to_string(state_path).expect("state file");
    let key = dunce::canonicalize(&edit_dir).expect("canonical edit dir").display().to_string();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).expect("valid JSON"),
        json!({
            key: {
                "patchedPkg": "is-positive@1.0.0",
                "applyToAll": false,
            },
        }),
    );
}

#[test]
fn patch_state_write_updates_existing_state_file() {
    let tmp = tempdir().expect("temp dir");
    let modules_dir = tmp.path().join("node_modules");
    let first_edit_dir = tmp.path().join("first-edit");
    let second_edit_dir = tmp.path().join("second-edit");
    fs::create_dir_all(&first_edit_dir).expect("create first edit dir");
    fs::create_dir_all(&second_edit_dir).expect("create second edit dir");

    write_edit_dir_state::<Host>(&modules_dir, &first_edit_dir, &sample_state()).unwrap();
    write_edit_dir_state::<Host>(
        &modules_dir,
        &second_edit_dir,
        &EditDirState {
            patched_pkg: "is-negative@1.0.0".to_string(),
            apply_to_all: true,
            package_key: None,
        },
    )
    .unwrap();

    let first_key = dunce::canonicalize(&first_edit_dir)
        .expect("canonical first edit dir")
        .display()
        .to_string();
    let second_key = dunce::canonicalize(&second_edit_dir)
        .expect("canonical second edit dir")
        .display()
        .to_string();
    let state_path = modules_dir.join(".pnpm_patches").join("state.json");
    let text = fs::read_to_string(state_path).expect("state file");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).expect("valid JSON"),
        json!({
            first_key: {
                "patchedPkg": "is-positive@1.0.0",
                "applyToAll": false,
            },
            second_key: {
                "patchedPkg": "is-negative@1.0.0",
                "applyToAll": true,
            },
        }),
    );
}

#[test]
fn patch_state_atomic_writer_replaces_existing_file() {
    let tmp = tempdir().expect("temp dir");
    let state_file = tmp.path().join("state.json");
    fs::write(&state_file, "old").expect("write old state");

    write_state_file_atomically(&state_file, b"new").expect("replace state");

    assert_eq!(fs::read_to_string(state_file).expect("read state"), "new");
}

#[test]
fn patch_state_read_uses_resolved_edit_dir_key() {
    let tmp = tempdir().expect("temp dir");
    let modules_dir = tmp.path().join("node_modules");
    let edit_dir = tmp.path().join("edit");
    fs::create_dir_all(&edit_dir).expect("create edit dir");

    write_edit_dir_state::<Host>(&modules_dir, &edit_dir, &sample_state()).unwrap();

    let edit_dir_with_dot = tmp.path().join(".").join("edit");
    assert_eq!(
        read_edit_dir_state::<Host>(&modules_dir, &edit_dir_with_dot).unwrap(),
        Some(sample_state()),
    );
}

#[test]
fn patch_state_malformed_json_is_an_error() {
    let tmp = tempdir().expect("temp dir");
    let modules_dir = tmp.path().join("node_modules");
    let state_dir = modules_dir.join(".pnpm_patches");
    fs::create_dir_all(&state_dir).expect("create state dir");
    fs::write(state_dir.join("state.json"), "{").expect("write malformed state");

    let err = read_edit_dir_state::<Host>(&modules_dir, &tmp.path().join("edit")).unwrap_err();
    assert!(err.to_string().contains("state.json"), "error includes state path: {err}");
}

#[test]
fn patch_state_read_errors_when_state_path_is_not_a_file() {
    let tmp = tempdir().expect("temp dir");
    let modules_dir = tmp.path().join("node_modules");
    let state_path = modules_dir.join(".pnpm_patches").join("state.json");
    fs::create_dir_all(&state_path).expect("create state dir at file path");

    let err = read_edit_dir_state::<Host>(&modules_dir, &tmp.path().join("edit")).unwrap_err();

    assert!(matches!(err, StateFileError::Read { .. }));
}

#[test]
fn patch_state_read_rejects_oversized_state_file() {
    let tmp = tempdir().expect("temp dir");
    let modules_dir = tmp.path().join("node_modules");
    let state_path = modules_dir.join(".pnpm_patches").join("state.json");
    fs::create_dir_all(state_path.parent().expect("state parent")).expect("create state dir");
    fs::write(&state_path, " ".repeat(super::MAX_STATE_FILE_BYTES + 1))
        .expect("write oversized state");

    let err = read_edit_dir_state::<Host>(&modules_dir, &tmp.path().join("edit")).unwrap_err();

    assert!(matches!(err, StateFileError::StateFileTooLarge { .. }));
}

#[test]
fn patch_state_write_errors_when_existing_state_path_is_not_a_file() {
    let tmp = tempdir().expect("temp dir");
    let modules_dir = tmp.path().join("node_modules");
    let state_path = modules_dir.join(".pnpm_patches").join("state.json");
    fs::create_dir_all(&state_path).expect("create state dir at file path");

    let err = write_edit_dir_state::<Host>(&modules_dir, &tmp.path().join("edit"), &sample_state())
        .unwrap_err();

    assert!(matches!(err, StateFileError::Read { .. }));
}

#[cfg(unix)]
#[test]
fn patch_state_write_rejects_symlinked_state_dir() {
    let tmp = tempdir().expect("temp dir");
    let modules_dir = tmp.path().join("node_modules");
    let state_dir = modules_dir.join(".pnpm_patches");
    let outside_dir = tmp.path().join("outside");
    fs::create_dir_all(&modules_dir).expect("create modules dir");
    fs::create_dir(&outside_dir).expect("create outside dir");
    std::os::unix::fs::symlink(&outside_dir, &state_dir).expect("symlink state dir");

    let err = write_edit_dir_state::<Host>(&modules_dir, &tmp.path().join("edit"), &sample_state())
        .unwrap_err();

    assert!(matches!(err, StateFileError::UnsafePath { .. }));
    assert!(!outside_dir.join("state.json").exists(), "outside state file must not be written");
}

#[cfg(unix)]
#[test]
fn patch_state_write_rejects_symlinked_state_file() {
    let tmp = tempdir().expect("temp dir");
    let modules_dir = tmp.path().join("node_modules");
    let state_dir = modules_dir.join(".pnpm_patches");
    let state_path = state_dir.join("state.json");
    let outside_target = tmp.path().join("outside-state.json");
    fs::create_dir_all(&state_dir).expect("create state dir");
    fs::write(&outside_target, "{}").expect("write outside state");
    std::os::unix::fs::symlink(&outside_target, &state_path).expect("symlink state file");

    let err = write_edit_dir_state::<Host>(&modules_dir, &tmp.path().join("edit"), &sample_state())
        .unwrap_err();

    assert!(matches!(err, StateFileError::UnsafePath { .. }));
    assert_eq!(fs::read_to_string(&outside_target).expect("read outside state"), "{}");
}

/// [`GetCurrentDir`] provider pinned to a fixed path, so the relative
/// edit-dir branches run without mutating the process cwd (which would
/// race the rest of the parallel test suite).
struct FakeCwd;

static FAKE_CWD: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

impl GetCurrentDir for FakeCwd {
    fn current_dir() -> io::Result<PathBuf> {
        Ok(FAKE_CWD.get().expect("FAKE_CWD initialized by the test").clone())
    }
}

/// [`GetCurrentDir`] provider whose cwd is unavailable, standing in for
/// a deleted current directory.
struct NoCwd;

impl GetCurrentDir for NoCwd {
    fn current_dir() -> io::Result<PathBuf> {
        Err(io::Error::from(io::ErrorKind::NotFound))
    }
}

#[test]
fn patch_state_write_uses_current_dir_for_relative_edit_dirs() {
    let tmp = tempdir().expect("temp dir");
    let cwd = dunce::canonicalize(tmp.path()).expect("canonical temp dir");
    FAKE_CWD.set(cwd.clone()).expect("set FAKE_CWD once");

    let modules_dir = cwd.join("node_modules");
    fs::create_dir(cwd.join("edit")).expect("create relative edit dir");

    write_edit_dir_state::<FakeCwd>(&modules_dir, Path::new("edit"), &sample_state()).unwrap();

    let state_path = modules_dir.join(".pnpm_patches").join("state.json");
    let text = fs::read_to_string(state_path).expect("state file");
    let key =
        dunce::canonicalize(cwd.join("edit")).expect("canonical edit dir").display().to_string();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).expect("valid JSON"),
        json!({
            key: {
                "patchedPkg": "is-positive@1.0.0",
                "applyToAll": false,
            },
        }),
    );
}

#[test]
fn patch_state_reports_current_dir_resolution_errors_for_relative_edit_dirs() {
    let err = edit_dir_key::<NoCwd>(Path::new("edit")).expect_err("unavailable cwd should fail");

    assert!(matches!(err, StateFileError::ResolveEditDir { .. }));
}

#[test]
fn patch_state_write_uses_normalized_missing_edit_dir_key() {
    let tmp = tempdir().expect("temp dir");
    let modules_dir = tmp.path().join("node_modules");
    let missing_edit_dir = tmp.path().join("missing-edit");

    write_edit_dir_state::<Host>(&modules_dir, &missing_edit_dir, &sample_state()).unwrap();

    let state_path = modules_dir.join(".pnpm_patches").join("state.json");
    let text = fs::read_to_string(state_path).expect("state file");
    assert!(
        serde_json::from_str::<serde_json::Value>(&text).expect("valid JSON")
            [&missing_edit_dir.display().to_string()]
            .is_object(),
    );
}

#[cfg(unix)]
#[test]
fn patch_state_errors_when_edit_dir_parent_is_not_a_directory() {
    let tmp = tempdir().expect("temp dir");
    let file_parent = tmp.path().join("file-parent");
    fs::write(&file_parent, "").expect("write file parent");

    let err = edit_dir_key::<Host>(&file_parent.join("edit")).expect_err("file parent should fail");

    assert!(matches!(err, StateFileError::ResolveEditDir { .. }));
}
