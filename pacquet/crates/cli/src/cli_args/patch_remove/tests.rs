use super::{
    DialoguerPatchRemovePrompt, PatchRemovalContext, PatchRemovalTarget, PatchRemoveArgs,
    PatchRemoveError, PatchRemoveFs, PatchRemovePrompt, join_setting_path,
    patches_from_selected_indices, patches_to_remove, remove_empty_patch_dirs,
    remove_empty_patch_dirs_with_fs, select_patches_from_indices, unlink_patch_if_exists,
};
use crate::State;
use indexmap::IndexMap;
use std::{io::IsTerminal, path::Path};

#[test]
fn explicit_patch_args_skip_prompt() {
    let selected = patches_to_remove(
        vec!["pkg".to_string()],
        &IndexMap::new(),
        &FakePrompt { selected: vec![], called: std::cell::Cell::new(false) },
    )
    .expect("selected patches");

    assert_eq!(selected, vec!["pkg"]);
}

#[test]
fn empty_patch_args_prompt_for_available_patches() {
    let patched_dependencies =
        IndexMap::from([("pkg".to_string(), "patches/pkg.patch".to_string())]);
    let prompt =
        FakePrompt { selected: vec!["pkg".to_string()], called: std::cell::Cell::new(false) };

    let selected =
        patches_to_remove(Vec::new(), &patched_dependencies, &prompt).expect("selected patch");

    assert_eq!(selected, vec!["pkg"]);
    assert!(prompt.called.get());
}

#[test]
fn empty_patch_args_without_patches_errors() {
    let err = patches_to_remove(
        Vec::new(),
        &IndexMap::new(),
        &FakePrompt { selected: vec![], called: std::cell::Cell::new(false) },
    )
    .expect_err("no patches should error");

    assert!(matches!(err, PatchRemoveError::NoPatchesToRemove));
}

#[test]
fn empty_prompt_selection_is_treated_as_no_patches_to_remove() {
    let patched_dependencies =
        IndexMap::from([("pkg".to_string(), "patches/pkg.patch".to_string())]);

    let err = patches_to_remove(
        Vec::new(),
        &patched_dependencies,
        &FakePrompt { selected: vec![], called: std::cell::Cell::new(false) },
    )
    .expect_err("empty prompt selection should error");

    assert!(matches!(err, PatchRemoveError::NoPatchesToRemove));
}

#[test]
fn selected_prompt_indices_are_mapped_to_patch_names() {
    let patches = vec!["first".to_string(), "second".to_string(), "third".to_string()];

    assert_eq!(
        patches_from_selected_indices(&patches, vec![2, 0]),
        vec!["third".to_string(), "first".to_string()],
    );
}

#[test]
fn select_patches_from_indices_maps_selected_indices() {
    let patches = vec!["first".to_string(), "second".to_string(), "third".to_string()];

    let selected = select_patches_from_indices(&patches, |_| Ok(vec![1, 2]))
        .expect("selected patches from indices");

    assert_eq!(selected, vec!["second".to_string(), "third".to_string()]);
}

#[test]
fn dialoguer_prompt_reports_cancellation_when_stdin_is_not_interactive() {
    assert!(!std::io::stdin().is_terminal(), "test requires non-interactive stdin");

    let prompt = DialoguerPatchRemovePrompt;
    let err = prompt.select_patches(&["pkg".to_string()]).expect_err("prompt should cancel");

    assert!(matches!(err, PatchRemoveError::Canceled));
}

#[test]
fn patch_removal_context_rejects_patches_dir_outside_project() {
    let tmp = tempfile::tempdir().expect("temp dir");

    let Err(err) = PatchRemovalContext::new(tmp.path(), "../patches") else {
        panic!("outside patches dir should error");
    };

    assert!(matches!(err, PatchRemoveError::PatchesDirOutsideProject { .. }));
}

#[cfg(unix)]
#[test]
fn patch_removal_context_rejects_real_patches_dir_symlink_outside_project() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let project = tmp.path().join("project");
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&project).expect("create project");
    std::fs::create_dir_all(&outside).expect("create outside");
    std::os::unix::fs::symlink(&outside, project.join("patches")).expect("symlink patches dir");

    let Err(err) = PatchRemovalContext::new(&project, "patches") else {
        panic!("symlinked outside patches dir should error");
    };

    assert!(matches!(err, PatchRemoveError::PatchesDirOutsideProject { .. }));
}

#[tokio::test]
async fn run_rejects_configured_patches_dir_outside_project() {
    let tmp = tempfile::tempdir().expect("temp dir");
    std::fs::write(tmp.path().join("package.json"), "{}").expect("write package.json");

    let mut config = pacquet_config::Config::new();
    config.workspace_dir = Some(tmp.path().to_path_buf());
    config.patches_dir = Some("../patches".to_string());
    config.patched_dependencies =
        Some(IndexMap::from([("pkg@1.0.0".to_string(), "patches/pkg.patch".to_string())]));
    let config: &'static pacquet_config::Config = Box::leak(Box::new(config));
    let state = State {
        tarball_mem_cache: std::sync::Arc::new(pacquet_tarball::MemCache::default()),
        http_client: std::sync::Arc::new(pacquet_network::ThrottledClient::default()),
        config,
        manifest: pacquet_package_manifest::PackageManifest::from_path(
            tmp.path().join("package.json"),
        )
        .expect("package manifest"),
        lockfile: pacquet_lockfile::LazyLockfile::disabled(),
        resolved_packages: pacquet_package_manager::ResolvedPackages::new(),
    };

    let err = PatchRemoveArgs { patches: vec!["pkg@1.0.0".to_string()] }
        .run(tmp.path(), state)
        .await
        .expect_err("outside patches dir should error");

    assert!(matches!(err, PatchRemoveError::PatchesDirOutsideProject { .. }));
}

#[tokio::test]
async fn run_keeps_patch_file_still_used_by_remaining_entries() {
    let tmp = tempfile::tempdir().expect("temp dir");
    std::fs::write(tmp.path().join("package.json"), "{}").expect("write package.json");
    let patch_file = tmp.path().join("patches/shared.patch");
    std::fs::create_dir_all(patch_file.parent().expect("patch parent"))
        .expect("create patches dir");
    std::fs::write(&patch_file, "shared patch").expect("write shared patch");

    let mut config = pacquet_config::Config::new();
    config.workspace_dir = Some(tmp.path().to_path_buf());
    config.patched_dependencies = Some(IndexMap::from([
        ("first@1.0.0".to_string(), "patches/shared.patch".to_string()),
        ("second@1.0.0".to_string(), "patches/shared.patch".to_string()),
    ]));
    let config: &'static pacquet_config::Config = Box::leak(Box::new(config));
    let state = State {
        tarball_mem_cache: std::sync::Arc::new(pacquet_tarball::MemCache::default()),
        http_client: std::sync::Arc::new(pacquet_network::ThrottledClient::default()),
        config,
        manifest: pacquet_package_manifest::PackageManifest::from_path(
            tmp.path().join("package.json"),
        )
        .expect("package manifest"),
        lockfile: pacquet_lockfile::LazyLockfile::disabled(),
        resolved_packages: pacquet_package_manager::ResolvedPackages::new(),
    };

    PatchRemoveArgs { patches: vec!["first@1.0.0".to_string()] }
        .run(tmp.path(), state)
        .await
        .expect("remove first patch entry");

    assert_eq!(std::fs::read_to_string(&patch_file).expect("shared patch file"), "shared patch");
}

#[test]
fn patch_removal_target_rejects_patch_file_outside_patches_dir() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let ctx = PatchRemovalContext::new(tmp.path(), "patches").expect("context");

    let Err(err) = PatchRemovalTarget::new("pkg", "other/pkg.patch", &ctx) else {
        panic!("patch file outside patches dir should error");
    };

    assert!(matches!(err, PatchRemoveError::PatchFileOutsidePatchesDir { .. }));
}

#[test]
fn patch_removal_target_rejects_directory_entries() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let patches = tmp.path().join("patches");
    std::fs::create_dir_all(patches.join("pkg.patch")).expect("create directory patch target");
    let ctx = PatchRemovalContext::new(tmp.path(), "patches").expect("context");

    let Err(err) = PatchRemovalTarget::new("pkg", "patches/pkg.patch", &ctx) else {
        panic!("directory patch target should error");
    };

    assert!(matches!(err, PatchRemoveError::PatchFileIsDirectory { .. }));
}

#[cfg(unix)]
#[test]
fn patch_removal_target_reports_lstat_errors() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let patches = tmp.path().join("patches");
    std::fs::create_dir_all(&patches).expect("create patches dir");
    std::fs::write(patches.join("not-a-dir"), "file").expect("write file parent");
    let ctx = PatchRemovalContext::new(tmp.path(), "patches").expect("context");

    let Err(err) = PatchRemovalTarget::new("pkg", "patches/not-a-dir/pkg.patch", &ctx) else {
        panic!("non-directory parent should error");
    };

    assert!(matches!(err, PatchRemoveError::ReadPatchDir { .. }));
}

#[test]
fn join_setting_path_ignores_root_and_current_dir_components() {
    let tmp = tempfile::tempdir().expect("temp dir");

    assert_eq!(
        join_setting_path(tmp.path(), "./patches/./nested"),
        tmp.path().join("patches").join("nested"),
    );

    #[cfg(unix)]
    assert_eq!(join_setting_path(tmp.path(), "/patches"), tmp.path().join("patches"));
}

#[test]
fn unlink_patch_if_exists_ignores_missing_existing_target() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let target = PatchRemovalTarget {
        patch: "pkg".to_string(),
        parent_dir: tmp.path().join("patches"),
        target_path: tmp.path().join("patches/pkg.patch"),
        target_exists: true,
    };

    unlink_patch_if_exists(&target).expect("missing target is ignored");
}

#[test]
fn unlink_patch_if_exists_reports_remove_errors() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let target_path = tmp.path().join("patches/pkg.patch");
    std::fs::create_dir_all(&target_path).expect("create directory at target path");
    let target = PatchRemovalTarget {
        patch: "pkg".to_string(),
        parent_dir: tmp.path().join("patches"),
        target_path,
        target_exists: true,
    };

    let err = unlink_patch_if_exists(&target).expect_err("directory target should fail to unlink");

    assert!(matches!(err, PatchRemoveError::RemovePatchFile { .. }));
}

#[test]
fn remove_empty_patch_dirs_removes_empty_dirs_and_ignores_missing_dirs() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let empty = tmp.path().join("patches").join("nested");
    std::fs::create_dir_all(&empty).expect("create empty patch dir");
    let missing = tmp.path().join("patches").join("missing");

    remove_empty_patch_dirs(&[
        PatchRemovalTarget {
            patch: "empty".to_string(),
            parent_dir: empty.clone(),
            target_path: empty.join("pkg.patch"),
            target_exists: false,
        },
        PatchRemovalTarget {
            patch: "missing".to_string(),
            parent_dir: missing,
            target_path: tmp.path().join("patches/missing/pkg.patch"),
            target_exists: false,
        },
    ])
    .expect("remove empty dirs");

    assert!(!empty.exists(), "empty patch dir should be removed");
}

#[test]
fn remove_empty_patch_dirs_reports_read_errors() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let parent = tmp.path().join("patches");
    std::fs::write(&parent, "not a directory").expect("write file parent");

    let err = remove_empty_patch_dirs(&[PatchRemovalTarget {
        patch: "pkg".to_string(),
        parent_dir: parent,
        target_path: tmp.path().join("patches/pkg.patch"),
        target_exists: false,
    }])
    .expect_err("file parent should error");

    assert!(matches!(err, PatchRemoveError::ReadPatchDir { .. }));
}

#[test]
fn remove_empty_patch_dirs_reports_remove_errors() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let empty = tmp.path().join("patches").join("nested");

    let result = remove_empty_patch_dirs_with_fs(
        &[PatchRemovalTarget {
            patch: "pkg".to_string(),
            parent_dir: empty.clone(),
            target_path: empty.join("pkg.patch"),
            target_exists: false,
        }],
        &RemoveDirErrorFs,
    );

    assert!(matches!(result, Err(PatchRemoveError::RemovePatchDir { .. })));
}

#[test]
fn remove_empty_patch_dirs_keeps_non_empty_dirs() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let parent = tmp.path().join("patches").join("nested");

    remove_empty_patch_dirs_with_fs(
        &[PatchRemovalTarget {
            patch: "pkg".to_string(),
            parent_dir: parent.clone(),
            target_path: parent.join("pkg.patch"),
            target_exists: false,
        }],
        &NonEmptyDirFs,
    )
    .expect("keep non-empty dir");
}

struct FakePrompt {
    selected: Vec<String>,
    called: std::cell::Cell<bool>,
}

impl PatchRemovePrompt for FakePrompt {
    fn select_patches(&self, _patches: &[String]) -> Result<Vec<String>, PatchRemoveError> {
        self.called.set(true);
        Ok(self.selected.clone())
    }
}

struct RemoveDirErrorFs;

impl PatchRemoveFs for RemoveDirErrorFs {
    fn is_dir_empty(&self, _path: &Path) -> std::io::Result<bool> {
        Ok(true)
    }

    fn remove_dir(&self, _path: &Path) -> std::io::Result<()> {
        Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "blocked remove_dir"))
    }
}

struct NonEmptyDirFs;

impl PatchRemoveFs for NonEmptyDirFs {
    fn is_dir_empty(&self, _path: &Path) -> std::io::Result<bool> {
        Ok(false)
    }

    fn remove_dir(&self, _path: &Path) -> std::io::Result<()> {
        panic!("non-empty dirs should not be removed")
    }
}
