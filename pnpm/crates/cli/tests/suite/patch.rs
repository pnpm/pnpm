use crate::_utils;

use _utils::append_workspace_yaml_key;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_patching::create_hex_hash_from_file;
use pnpm_store_dir::{StoreDir, StoreIndex};
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
    git_repo::GitRepoFixture,
};
use serde_json::Value;
use std::{ffi::OsStr, fmt::Write as _, fs, path::Path, process::Command};
use tempfile::TempDir;

const IS_POSITIVE_PATCH: &str = include_str!(
    "../../../../../pnpm11/installing/deps-installer/test/fixtures/patch-pkg/is-positive@1.0.0.patch"
);

/// Adds a `postinstall` script the published package does not declare,
/// plus the script it runs.
const IS_POSITIVE_POSTINSTALL_PATCH: &str = include_str!(
    "../../../../../pnpm11/installing/deps-installer/test/fixtures/patch-pkg/is-positive@1.0.0-postinstall.patch"
);

/// Creates a `binding.gyp`, which the lifecycle runner turns into an
/// implicit `node-gyp rebuild`.
const IS_POSITIVE_BINDING_GYP_PATCH: &str = include_str!(
    "../../../../../pnpm11/installing/deps-installer/test/fixtures/patch-pkg/is-positive@1.0.0-binding-gyp.patch"
);

/// Creates a plain file named `.hooks`, which is not a hook directory.
const IS_POSITIVE_HOOKS_FILE_PATCH: &str = include_str!(
    "../../../../../pnpm11/installing/deps-installer/test/fixtures/patch-pkg/is-positive@1.0.0-hooks-file.patch"
);

/// Adds a marker file, so a package's patched state can be read off the
/// filesystem without depending on the package's own sources.
const MARKER_PATCH: &str = concat!(
    "diff --git a/patched-marker.txt b/patched-marker.txt\n",
    "new file mode 100644\n",
    "index 0000000..3f2e1d4\n",
    "--- /dev/null\n",
    "+++ b/patched-marker.txt\n",
    "@@ -0,0 +1 @@\n",
    "+patched\n",
);

fn setup_installed() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "is-positive": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    pacquet(&workspace, ["install"]).assert().success();
    (root, workspace, npmrc_info)
}

fn setup_installed_workspace_project()
-> (TempDir, std::path::PathBuf, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "0.0.0",
            "private": true,
        })
        .to_string(),
    )
    .expect("write root package.json");
    let app_dir = workspace.join("packages/app");
    fs::create_dir_all(&app_dir).expect("create workspace app");
    fs::write(
        app_dir.join("package.json"),
        serde_json::json!({
            "name": "app",
            "version": "0.0.0",
            "dependencies": {
                "is-positive": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write app package.json");
    pacquet(&workspace, ["install"]).assert().success();
    (root, workspace, app_dir, npmrc_info)
}

fn setup_configured_patch(
    patch_key: &str,
    patch_file_name: &str,
) -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    setup_configured_patch_with_yaml(patch_key, patch_file_name, "")
}

fn setup_configured_patch_with_yaml(
    patch_key: &str,
    patch_file_name: &str,
    extra_yaml: &str,
) -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "is-positive": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::create_dir_all(workspace.join("patches")).expect("create patches dir");
    fs::write(workspace.join("patches").join(patch_file_name), IS_POSITIVE_PATCH)
        .expect("write patch file");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    writeln!(&mut workspace_yaml, "patchedDependencies:\n  {patch_key}: patches/{patch_file_name}")
        .expect("append patchedDependencies");
    workspace_yaml.push_str(extra_yaml);
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");
    (root, workspace, npmrc_info)
}

fn setup_patch_remove_project(
    entries: &[(&str, &str)],
) -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "is-positive": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("patchedDependencies:\n");
    for (key, patch_file) in entries {
        writeln!(&mut workspace_yaml, "  {key}: {patch_file}")
            .expect("append patchedDependencies entry");
    }
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");
    (root, workspace, npmrc_info)
}

fn pacquet(workspace: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_args(args)
}

fn patch_state(workspace: &Path) -> Value {
    let state_path = workspace.join("node_modules/.pnpm_patches/state.json");
    serde_json::from_str(&fs::read_to_string(state_path).expect("read patch state"))
        .expect("parse patch state")
}

fn write_patch_edit(edit_dir: &Path, marker: &str) {
    fs::write(
        edit_dir.join("index.js"),
        format!("module.exports = function () {{ return {marker:?} }}\n"),
    )
    .expect("edit package file");
}

fn remove_dir_if_exists(path: &Path) {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("remove {}: {error}", path.display()),
    }
}

fn installed_is_a_symlink(workspace: &Path) -> bool {
    is_symlink_or_junction(&workspace.join("node_modules/is-positive"))
        .expect("stat node_modules/is-positive")
}

fn read_installed_index(workspace: &Path) -> String {
    fs::read_to_string(workspace.join("node_modules/is-positive/index.js"))
        .expect("read the installed is-positive/index.js")
}

fn patch_file_hash(workspace: &Path, patch_file_name: &str) -> String {
    create_hex_hash_from_file(&workspace.join("patches").join(patch_file_name))
        .expect("hash the patch file")
}

fn read_wanted_lockfile(workspace: &Path) -> pnpm_lockfile::Lockfile {
    let text = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    serde_saphyr::from_str(&text).expect("parse pnpm-lock.yaml")
}

fn snapshot_keys(lockfile: &pnpm_lockfile::Lockfile) -> Vec<String> {
    let mut keys: Vec<String> = lockfile
        .snapshots
        .as_ref()
        .expect("the lockfile records snapshots")
        .keys()
        .map(ToString::to_string)
        .collect();
    keys.sort();
    keys
}

/// The store index row `is-positive@1.0.0` hangs off, keyed by the peer-
/// and patch-free package id.
fn is_positive_store_row(store_dir: &Path) -> pnpm_store_dir::PackageFilesIndex {
    let store = StoreDir::new(store_dir);
    let index = StoreIndex::open_readonly_in(&store).expect("open the store index");
    let row_key = index
        .keys()
        .expect("list the store index keys")
        .into_iter()
        .find(|key| key.ends_with("\tis-positive@1.0.0"))
        .expect("a store index row for is-positive@1.0.0");
    index.get(&row_key).expect("read the store index row").expect("the row is present")
}

/// Assert the store kept the patched `index.js` as a side-effects overlay
/// rather than overwriting the pristine one it shares with every other
/// project. The overlay's cache key ends in `;patch=<hash>` — pacquet
/// composes it in `pnpm_graph_hasher::calc_dep_state`, and the row it
/// hangs off is keyed by the peer- and patch-free `is-positive@1.0.0`.
fn assert_patched_side_effects_cached(store_dir: &Path, patch_hash: &str) {
    let row = is_positive_store_row(store_dir);

    let side_effects =
        row.side_effects.as_ref().expect("a patched package populates `sideEffects`");
    let key_suffix = format!(";patch={patch_hash}");
    let diff = side_effects
        .iter()
        .find_map(|(cache_key, diff)| cache_key.ends_with(&key_suffix).then_some(diff))
        .unwrap_or_else(|| {
            panic!(
                "no `{key_suffix}` cache key among {:?}",
                side_effects.keys().collect::<Vec<_>>(),
            )
        });

    let cached = diff
        .added
        .as_ref()
        .expect("the patched files land in `added`")
        .get("index.js")
        .expect("the patched index.js is cached");
    let pristine = row.files.get("index.js").expect("the pristine index.js is indexed");
    assert_ne!(
        pristine.digest, cached.digest,
        "the patched file must not share the pristine file's digest",
    );
}

/// Install `is-positive@1.0.0` in a sibling project that shares the store
/// but configures no patches, and return the `index.js` it received.
///
/// The install is offline so the files can only have come from the store
/// the patched project just wrote to.
fn install_unpatched_sibling(root: &Path, extra_yaml: &str) -> String {
    let project = root.join("unpatched-project");
    fs::create_dir_all(&project).expect("create the unpatched project");
    // Both projects are direct children of the temp root, so the store and
    // cache paths the harness wrote as `../pacquet-*` resolve the same way.
    fs::copy(root.join("workspace/.npmrc"), project.join(".npmrc")).expect("copy .npmrc");
    fs::write(
        project.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "is-positive": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    let mut workspace_yaml = String::from(concat!(
        "storeDir: ../pacquet-store\n",
        "cacheDir: ../pacquet-cache\n",
        "enableGlobalVirtualStore: false\n",
        "offline: true\n",
    ));
    workspace_yaml.push_str(extra_yaml);
    fs::write(project.join("pnpm-workspace.yaml"), workspace_yaml)
        .expect("write pnpm-workspace.yaml");

    pacquet(&project, ["install", "--reporter=silent"]).assert().success();
    read_installed_index(&project)
}

/// The scenario shared by the four upstream `patch.ts` install tests
/// (`:24`, `:120`, `:297`, `:386`), which differ only in the patch key and
/// the build-related settings in `extra_yaml`: a fresh install applies the
/// patch and records it in both the lockfile and the side-effects cache, a
/// frozen reinstall replays it under each node linker, and a project that
/// shares the store without configuring patches still gets pristine files.
fn assert_patch_install_scenario(patch_key: &str, patch_file_name: &str, extra_yaml: &str) {
    // Hardlinking is what makes the unpatched-sibling check below bite:
    // it is the import method under which a patch written in place would
    // mutate the inode the store shares with every other project. The
    // default `auto` reflinks on a copy-on-write filesystem, which hides
    // that class of bug. Upstream pins the same method for this scenario.
    let settings = format!("packageImportMethod: hardlink\n{extra_yaml}");
    let (root, workspace, npmrc_info) =
        setup_configured_patch_with_yaml(patch_key, patch_file_name, &settings);
    let AddMockedRegistry { mock_instance, store_dir, .. } = npmrc_info;

    pacquet(&workspace, ["install", "--reporter=silent"]).assert().success();

    let installed = read_installed_index(&workspace);
    assert!(installed.contains("// patched"), "installed: {installed}");

    let patch_hash = patch_file_hash(&workspace, patch_file_name);
    let lockfile = read_wanted_lockfile(&workspace);
    assert_eq!(
        lockfile.patched_dependencies,
        Some(std::iter::once((patch_key.to_string(), patch_hash.clone())).collect()),
    );
    let patched_snapshot = format!("is-positive@1.0.0(patch_hash={patch_hash})");
    let snapshots = snapshot_keys(&lockfile);
    assert!(snapshots.contains(&patched_snapshot), "snapshots: {snapshots:?}");

    assert_patched_side_effects_cached(&store_dir, &patch_hash);

    remove_dir_if_exists(&workspace.join("node_modules"));
    pacquet(&workspace, ["install", "--frozen-lockfile", "--reporter=silent"]).assert().success();
    let replayed = read_installed_index(&workspace);
    assert!(replayed.contains("// patched"), "replayed: {replayed}");
    assert!(
        installed_is_a_symlink(&workspace),
        "the isolated linker symlinks into the virtual store",
    );

    remove_dir_if_exists(&workspace.join("node_modules"));
    append_workspace_yaml_key(&workspace, "nodeLinker", "hoisted");
    pacquet(&workspace, ["install", "--frozen-lockfile", "--reporter=silent"]).assert().success();
    let hoisted = read_installed_index(&workspace);
    assert!(hoisted.contains("// patched"), "hoisted: {hoisted}");
    assert!(!installed_is_a_symlink(&workspace), "the hoisted linker places a real directory");

    let unpatched = install_unpatched_sibling(root.path(), &settings);
    assert!(!unpatched.contains("// patched"), "unpatched sibling: {unpatched}");

    drop((root, mock_instance));
}

#[test]
fn patch_errors_when_package_is_missing() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let output = pacquet(&workspace, ["patch", "--reporter=silent"]).output().expect("run patch");

    assert!(!output.status.success(), "patch without package should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("requires the package name"), "stderr: {stderr}");

    drop((root, mock_instance));
}

#[test]
fn patch_missing_package_name_takes_precedence_over_edit_dir_checks() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let edit_dir = workspace.join("custom-edit");
    fs::create_dir_all(&edit_dir).expect("create edit dir");
    fs::write(edit_dir.join("index.js"), "already here").expect("seed edit dir");

    let output = pacquet(
        &workspace,
        ["patch", "--edit-dir", edit_dir.to_str().expect("utf8 edit dir"), "--reporter=silent"],
    )
    .output()
    .expect("run patch");

    assert!(!output.status.success(), "patch without package should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("requires the package name"), "stderr: {stderr}");
    assert!(!stderr.contains("already exists"), "stderr: {stderr}");

    drop((root, mock_instance));
}

#[test]
fn patch_errors_when_requested_version_is_not_installed() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let output = pacquet(&workspace, ["patch", "is-positive@2.0.0", "--reporter=silent"])
        .output()
        .expect("run patch");

    assert!(!output.status.success(), "missing installed version should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_PATCH_VERSION_NOT_FOUND"), "stderr: {stderr}");
    assert!(stderr.contains("1.0.0"), "stderr: {stderr}");

    drop((root, mock_instance));
}

/// The three upstream apply-failure tests (`patch.ts:508`, `:530`, `:552`)
/// differ only in how the patch key selects `is-positive@3.1.0`, whose
/// sources the `1.0.0` patch cannot apply to.
fn assert_patch_apply_failure(patch_key: &str) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "is-positive": "3.1.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::create_dir_all(workspace.join("patches")).expect("create patches dir");
    fs::write(workspace.join("patches/is-positive.patch"), IS_POSITIVE_PATCH)
        .expect("write patch file");
    append_workspace_yaml_key(
        &workspace,
        "patchedDependencies",
        format!("\n  {patch_key}: patches/is-positive.patch"),
    );

    let output = pacquet(&workspace, ["install"]).output().expect("run install");

    assert!(!output.status.success(), "an unappliable patch should fail the install");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_PATCH_FAILED"), "stderr: {stderr}");
    assert!(stderr.contains("Could not apply patch"), "stderr: {stderr}");
    let installed = read_installed_index(&workspace);
    assert!(!installed.contains("// patched"), "installed: {installed}");

    drop((root, mock_instance));
}

#[test]
fn patch_workflow_runs_with_default_ndjson_and_silent_reporters() {
    for reporter in [None, Some("--reporter=ndjson"), Some("--reporter=silent")] {
        let (root, workspace, npmrc_info) = setup_installed();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;
        let marker = match reporter {
            None => "patched default reporter",
            Some("--reporter=ndjson") => "patched ndjson reporter",
            Some("--reporter=silent") => "patched silent reporter",
            Some(other) => panic!("unexpected reporter {other}"),
        };

        let mut patch_cmd = pacquet(&workspace, ["patch", "is-positive@1.0.0"]);
        if let Some(reporter) = reporter {
            patch_cmd.arg(reporter);
        }
        patch_cmd.assert().success();

        let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
        write_patch_edit(&edit_dir, marker);

        let mut patch_commit_cmd =
            pacquet(&workspace, ["patch-commit", edit_dir.to_str().expect("utf8 edit dir")]);
        if let Some(reporter) = reporter {
            patch_commit_cmd.arg(reporter);
        }
        patch_commit_cmd.assert().success();

        let installed =
            fs::read_to_string(workspace.join("node_modules/is-positive/index.js")).unwrap();
        assert!(installed.contains(marker), "installed: {installed}");
        assert!(workspace.join("patches/is-positive@1.0.0.patch").is_file());

        let mut patch_remove_cmd = pacquet(&workspace, ["patch-remove", "is-positive@1.0.0"]);
        if let Some(reporter) = reporter {
            patch_remove_cmd.arg(reporter);
        }
        patch_remove_cmd.assert().success();

        let workspace_yaml =
            fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("workspace yaml");
        assert!(
            !workspace_yaml.contains("patchedDependencies:"),
            "workspace yaml: {workspace_yaml}",
        );
        assert!(!workspace.join("patches/is-positive@1.0.0.patch").exists());

        drop((root, mock_instance));
    }
}

#[test]
fn patch_reuses_existing_exact_patch_file_by_default() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();
    let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
    write_patch_edit(&edit_dir, "reused patch");
    pacquet(
        &workspace,
        ["patch-commit", edit_dir.to_str().expect("utf8 edit dir"), "--reporter=silent"],
    )
    .assert()
    .success();
    fs::remove_dir_all(&edit_dir).expect("remove edit dir");

    pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();

    let edited = fs::read_to_string(edit_dir.join("index.js")).expect("edit dir index");
    assert!(edited.contains("reused patch"), "edited: {edited}");

    drop((root, mock_instance));
}

#[test]
fn patch_ignore_existing_skips_existing_patch_file() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();
    let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
    write_patch_edit(&edit_dir, "ignored patch");
    pacquet(
        &workspace,
        ["patch-commit", edit_dir.to_str().expect("utf8 edit dir"), "--reporter=silent"],
    )
    .assert()
    .success();
    fs::remove_dir_all(&edit_dir).expect("remove edit dir");

    pacquet(&workspace, ["patch", "--ignore-existing", "is-positive@1.0.0", "--reporter=silent"])
        .assert()
        .success();

    let edited = fs::read_to_string(edit_dir.join("index.js")).expect("edit dir index");
    assert!(!edited.contains("ignored patch"), "edited: {edited}");

    drop((root, mock_instance));
}

#[test]
fn patch_errors_when_existing_patch_file_is_missing() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("patchedDependencies:\n  is-positive@1.0.0: patches/not-found.patch\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    let output = pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"])
        .output()
        .expect("run patch");

    assert!(!output.status.success(), "missing patch file should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_PATCH_FILE_NOT_FOUND"), "stderr: {stderr}");
    assert!(stderr.contains("Unable to find patch file"), "stderr: {stderr}");

    drop((root, mock_instance));
}

#[test]
fn patch_rejects_existing_patch_file_outside_patches_dir() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let outside_patch = workspace.parent().expect("workspace parent").join("outside.patch");
    fs::write(&outside_patch, IS_POSITIVE_PATCH).expect("write outside patch");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("patchedDependencies:\n  is-positive@1.0.0: ../outside.patch\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    let output = pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"])
        .output()
        .expect("run patch");

    assert!(!output.status.success(), "outside patch file should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_PATCH_FILE_OUTSIDE_PATCHES_DIR"), "stderr: {stderr}");

    drop((root, mock_instance));
}

#[test]
fn patch_exact_version_creates_edit_dir_and_state() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();

    let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
    assert!(edit_dir.join("package.json").is_file(), "edit dir package.json exists");
    assert!(edit_dir.join("index.js").is_file(), "edit dir package files exist");

    let key = dunce::canonicalize(&edit_dir).expect("canonical edit dir").display().to_string();
    let state = patch_state(&workspace);
    assert_eq!(state[&key]["patchedPkg"], "is-positive@1.0.0");
    assert_eq!(state[&key]["applyToAll"], false);
    assert_eq!(state[&key]["packageKey"], "is-positive@1.0.0");

    drop((root, mock_instance));
}

#[cfg(unix)]
#[test]
fn patch_rejects_symlinked_default_edit_root() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let outside_dir = root.path().join("outside-patch-edits");
    fs::create_dir(&outside_dir).expect("create outside edit dir");
    std::os::unix::fs::symlink(&outside_dir, workspace.join("node_modules/.pnpm_patches"))
        .expect("symlink default patch edit root");

    let output = pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"])
        .output()
        .expect("run patch");

    assert!(!output.status.success(), "symlinked default edit root should fail");
    assert!(
        !outside_dir.join("is-positive@1.0.0").exists(),
        "package files must not be extracted outside node_modules",
    );

    drop((root, mock_instance));
}

#[test]
fn patch_bare_name_single_version_sets_apply_to_all() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "is-positive", "--reporter=silent"]).assert().success();

    let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
    let key = dunce::canonicalize(&edit_dir).expect("canonical edit dir").display().to_string();
    let state = patch_state(&workspace);
    assert_eq!(state[&key]["patchedPkg"], "is-positive");
    assert_eq!(state[&key]["applyToAll"], true);
    assert_eq!(state[&key]["packageKey"], "is-positive@1.0.0");

    drop((root, mock_instance));
}

#[test]
fn patch_rejects_non_empty_edit_dir() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let edit_dir = workspace.join("custom-edit");
    fs::create_dir_all(&edit_dir).expect("create edit dir");
    fs::write(edit_dir.join("file.txt"), "already here").expect("seed edit dir");

    let output = pacquet(
        &workspace,
        [
            "patch",
            "--edit-dir",
            edit_dir.to_str().expect("utf8 path"),
            "is-positive@1.0.0",
            "--reporter=silent",
        ],
    )
    .output()
    .expect("run patch");

    assert!(!output.status.success(), "non-empty edit dir should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("target directory already exists"), "stderr: {stderr}");

    drop((root, mock_instance));
}

#[test]
fn patch_accepts_empty_custom_edit_dir() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let edit_dir = workspace.join("custom-edit");
    fs::create_dir_all(&edit_dir).expect("create edit dir");

    pacquet(
        &workspace,
        [
            "patch",
            "--edit-dir",
            edit_dir.to_str().expect("utf8 path"),
            "is-positive@1.0.0",
            "--reporter=silent",
        ],
    )
    .assert()
    .success();

    assert!(edit_dir.join("package.json").is_file(), "custom edit dir package.json exists");
    assert!(edit_dir.join("index.js").is_file(), "custom edit dir package file exists");

    let key = dunce::canonicalize(&edit_dir).expect("canonical edit dir").display().to_string();
    let state = patch_state(&workspace);
    assert_eq!(state[&key]["patchedPkg"], "is-positive@1.0.0");
    assert_eq!(state[&key]["packageKey"], "is-positive@1.0.0");

    drop((root, mock_instance));
}

fn setup_configured_patch_with_allow_unused(
    entries: &[(&str, &str)],
    allow_unused: bool,
) -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "is-positive": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::create_dir_all(workspace.join("patches")).expect("create patches dir");
    for (_key, file_name) in entries {
        fs::write(workspace.join("patches").join(file_name), IS_POSITIVE_PATCH)
            .expect("write patch file");
    }
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("patchedDependencies:\n");
    for (key, file_name) in entries {
        writeln!(&mut workspace_yaml, "  {key}: patches/{file_name}")
            .expect("append patchedDependencies entry");
    }
    if allow_unused {
        workspace_yaml.push_str("allowUnusedPatches: true\n");
    }
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");
    (root, workspace, npmrc_info)
}

fn setup_filtered_patch_workspace() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    fs::write(workspace.join("package.json"), serde_json::json!({}).to_string())
        .expect("write root package.json");
    let project = workspace.join("packages").join("pkg-a");
    fs::create_dir_all(&project).expect("create pkg-a dir");
    fs::write(
        project.join("package.json"),
        serde_json::json!({
            "name": "pkg-a",
            "dependencies": {
                "is-positive": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write pkg-a package.json");
    fs::create_dir_all(workspace.join("patches")).expect("create patches dir");
    fs::write(workspace.join("patches").join("is-positive@1.0.0.patch"), IS_POSITIVE_PATCH)
        .expect("write patch file");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str(concat!(
        "packages:\n",
        "  - 'packages/*'\n",
        "patchedDependencies:\n",
        "  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n",
    ));
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");
    (root, workspace, npmrc_info)
}

fn append_unused_filtered_patch(workspace: &Path) {
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    workspace_yaml.push_str("  is-negative@1.0.0: patches/is-positive@1.0.0.patch\n");
    fs::write(workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");
}

/// TS: `installing with no modules directory and a patched dependency`
/// (`deps-restorer/test/index.ts:848`). A headless install with
/// `enableModulesDir: false` must leave no `node_modules` directory
/// behind even when a dependency is patched.
#[test]
fn installing_with_no_modules_directory_and_a_patched_dependency() {
    let (root, workspace, npmrc_info) =
        setup_configured_patch("is-positive@1.0.0", "is-positive@1.0.0.patch");
    pacquet(&workspace, ["install", "--lockfile-only"]).assert().success();

    append_workspace_yaml_key(&workspace, "enableModulesDir", "false");
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    assert!(workspace.join("pnpm-lock.yaml").exists(), "the lockfile must still be written");
    assert!(
        !workspace.join("node_modules").exists(),
        "`enableModulesDir: false` must not create a node_modules directory",
    );

    drop((root, npmrc_info)); // cleanup
}

mod application;

mod commit;

mod removal;
