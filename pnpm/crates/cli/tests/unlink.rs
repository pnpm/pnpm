use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn write_manifest(workspace: &Path, manifest: &serde_json::Value) {
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
}

/// Scaffold a `sharedWorkspaceLockfile: false` workspace with two projects
/// (`packages/first`, `packages/second`) that both depend on `pkg`, plus a
/// `link:` override redirecting `pkg` to a local directory. Returns the two
/// project dirs. Callers run `unlink` *before* any install, so the link is
/// stripped and never materialized — the reinstall re-resolves `pkg` from the
/// registry.
fn scaffold_dedicated_link_workspace(workspace: &Path, pkg: &str) -> (PathBuf, PathBuf) {
    let local_target = workspace.join("local-dep");
    fs::create_dir_all(&local_target).expect("create local target dir");
    fs::write(
        local_target.join("package.json"),
        serde_json::json!({ "name": pkg, "version": "1.0.0" }).to_string(),
    )
    .expect("write local target package.json");

    let mut project_dirs = Vec::new();
    for name in ["first", "second"] {
        let dir = workspace.join("packages").join(name);
        fs::create_dir_all(&dir).expect("create project dir");
        fs::write(
            dir.join("package.json"),
            serde_json::json!({
                "name": name,
                "version": "1.0.0",
                "dependencies": { pkg: "100.0.0" },
            })
            .to_string(),
        )
        .expect("write project manifest");
        project_dirs.push(dir);
    }
    add_overrides(
        workspace,
        &format!(
            "packages:\n  - 'packages/*'\nsharedWorkspaceLockfile: false\noverrides:\n  '{pkg}': link:./local-dep\n",
        ),
    );
    let second = project_dirs.pop().expect("second project dir");
    let first = project_dirs.pop().expect("first project dir");
    (first, second)
}

/// Append an `overrides` block to the `pnpm-workspace.yaml` the mocked
/// registry already wrote (it carries `storeDir`/`cacheDir`, so the tests must
/// extend it rather than overwrite it). Pacquet (like pnpm) reads `overrides`
/// from `pnpm-workspace.yaml`, not from `package.json#pnpm.overrides`, so this
/// is where the link must be set up.
fn add_overrides(workspace: &Path, block: &str) {
    let path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&path).unwrap_or_default();
    if !yaml.is_empty() && !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str(block);
    fs::write(&path, yaml).expect("update pnpm-workspace.yaml");
}

fn read_workspace_yaml(workspace: &Path) -> String {
    fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read workspace yaml")
}

#[test]
fn unlink_removes_single_named_link_override() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "name": "test-project", "version": "1.0.0" }));
    add_overrides(&workspace, "overrides:\n  foo: link:../foo\n  bar: link:../bar\n  baz: 1.0.0\n");

    pacquet.with_args(["unlink", "foo"]).assert().success();

    let workspace_yaml = read_workspace_yaml(&workspace);
    assert!(!workspace_yaml.contains("foo"), "foo override must be removed: {workspace_yaml}");
    assert!(
        workspace_yaml.contains("bar: link:../bar"),
        "bar override must remain: {workspace_yaml}",
    );
    assert!(
        workspace_yaml.contains("baz: 1.0.0"),
        "non-link override must remain: {workspace_yaml}",
    );

    drop((root, mock_instance));
}

#[test]
fn unlink_without_args_removes_all_link_overrides() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "name": "test-project", "version": "1.0.0" }));
    add_overrides(&workspace, "overrides:\n  foo: link:../foo\n  bar: link:../bar\n  baz: 1.0.0\n");

    pacquet.with_arg("unlink").assert().success();

    let workspace_yaml = read_workspace_yaml(&workspace);
    assert!(!workspace_yaml.contains("foo"), "foo override must be removed: {workspace_yaml}");
    assert!(!workspace_yaml.contains("bar"), "bar override must be removed: {workspace_yaml}");
    assert!(
        workspace_yaml.contains("baz: 1.0.0"),
        "non-link override must remain: {workspace_yaml}",
    );

    drop((root, mock_instance));
}

#[test]
fn unlink_keeps_non_link_overrides() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "name": "test-project", "version": "1.0.0" }));
    add_overrides(&workspace, "overrides:\n  baz: 1.0.0\n");

    pacquet.with_args(["unlink", "baz"]).assert().success();

    let workspace_yaml = read_workspace_yaml(&workspace);
    assert!(
        workspace_yaml.contains("baz: 1.0.0"),
        "non-link override must remain untouched: {workspace_yaml}",
    );

    drop((root, mock_instance));
}

#[test]
fn unlink_is_noop_without_overrides() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "name": "test-project", "version": "1.0.0" }));

    let output = pacquet.with_arg("unlink").output().expect("run pacquet unlink");
    assert!(output.status.success(), "unlink without overrides must succeed");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Nothing to unlink"),
        "unlink must report when there is nothing to unlink, matching pnpm",
    );

    let workspace_yaml = read_workspace_yaml(&workspace);
    assert!(
        !workspace_yaml.contains("overrides"),
        "no overrides block should be created when there is nothing to unlink: {workspace_yaml}",
    );

    drop((root, mock_instance));
}

#[test]
fn unlink_drops_overrides_block_when_it_empties() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "name": "test-project", "version": "1.0.0" }));
    add_overrides(&workspace, "overrides:\n  foo: link:../foo\n  bar: link:../bar\n");

    pacquet.with_args(["unlink", "foo", "bar"]).assert().success();

    let workspace_yaml = read_workspace_yaml(&workspace);
    assert!(
        !workspace_yaml.contains("overrides:"),
        "the overrides block must be dropped once empty: {workspace_yaml}",
    );
    assert!(
        workspace_yaml.contains("storeDir"),
        "unrelated settings must be preserved: {workspace_yaml}",
    );

    drop((root, mock_instance));
}

#[test]
fn dislink_alias_works() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, &serde_json::json!({ "name": "test-project", "version": "1.0.0" }));
    add_overrides(&workspace, "overrides:\n  foo: link:../foo\n  baz: 1.0.0\n");

    pacquet.with_args(["dislink", "foo"]).assert().success();

    let workspace_yaml = read_workspace_yaml(&workspace);
    assert!(
        !workspace_yaml.contains("foo"),
        "foo must be removed via the dislink alias: {workspace_yaml}",
    );
    assert!(
        workspace_yaml.contains("baz: 1.0.0"),
        "non-link override must remain: {workspace_yaml}",
    );

    drop((root, mock_instance));
}

/// End-to-end: a `link:` override redirects a registry dependency to a local
/// directory, so installing produces a symlink in `node_modules` and a
/// `link:` resolution in the lockfile. After `unlink`, the override is gone
/// and the reinstall re-resolves the dependency from the registry — the
/// lockfile records the registry version instead of the link.
#[test]
fn unlink_restores_registry_package_in_lockfile() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    const PKG: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";

    let local_target = workspace.join("local-dep");
    fs::create_dir_all(&local_target).expect("create local target dir");
    fs::write(
        local_target.join("package.json"),
        serde_json::json!({ "name": PKG, "version": "1.0.0" }).to_string(),
    )
    .expect("write local target package.json");

    write_manifest(
        &workspace,
        &serde_json::json!({
            "name": "test-project",
            "version": "1.0.0",
            "dependencies": { PKG: "100.0.0" },
        }),
    );
    add_overrides(&workspace, &format!("overrides:\n  '{PKG}': link:./local-dep\n"));

    // Materialize the link first: the override makes node_modules a symlink
    // and pins a link: resolution in the lockfile.
    pacquet.with_arg("install").assert().success();
    let installed = workspace.join("node_modules").join(PKG);
    assert!(
        fs::symlink_metadata(&installed).is_ok_and(|meta| meta.file_type().is_symlink()),
        "the link override should make node_modules/{PKG} a symlink before unlink",
    );

    let mut unlink = std::process::Command::cargo_bin("pnpm").expect("locate pacquet binary");
    unlink.current_dir(&workspace);
    unlink.with_args(["unlink", PKG]).assert().success();

    let workspace_yaml = read_workspace_yaml(&workspace);
    assert!(
        !workspace_yaml.contains("link:"),
        "the link override must be removed from pnpm-workspace.yaml: {workspace_yaml}",
    );

    // The reinstall must re-resolve the dependency from the registry rather
    // than the local link, so the lockfile records the registry version.
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        lockfile.contains("dep-of-pkg-with-1-dep@100.0.0"),
        "lockfile must resolve the dependency from the registry after unlink: {lockfile}",
    );
    assert!(
        !lockfile.contains("link:"),
        "lockfile must not keep the link: resolution after unlink: {lockfile}",
    );

    drop((root, mock_instance));
}

/// `pnpm -r unlink` strips the workspace `link:` override once and reinstalls
/// every selected project, so in a `sharedWorkspaceLockfile: false` workspace
/// each project gets its own lockfile that re-resolves the dependency from the
/// registry.
#[test]
fn recursive_unlink_with_dedicated_lockfiles_reinstalls_each_project() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    const PKG: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
    let (first, second) = scaffold_dedicated_link_workspace(&workspace, PKG);

    let mut unlink = std::process::Command::cargo_bin("pnpm").expect("locate pacquet binary");
    unlink.current_dir(&workspace);
    unlink.with_args(["-r", "unlink", PKG]).assert().success();

    let workspace_yaml = read_workspace_yaml(&workspace);
    assert!(
        !workspace_yaml.contains("link:"),
        "the link override must be removed once: {workspace_yaml}",
    );
    for project in [&first, &second] {
        let lockfile = fs::read_to_string(project.join("pnpm-lock.yaml")).unwrap_or_else(|error| {
            panic!("each selected project must get its own lockfile ({error}): {project:?}")
        });
        assert!(
            lockfile.contains("dep-of-pkg-with-1-dep@100.0.0"),
            "{project:?} must re-resolve the dependency from the registry: {lockfile}",
        );
        assert!(
            !lockfile.contains("link:"),
            "{project:?} must not keep a link: resolution: {lockfile}",
        );
    }
    assert!(
        !workspace.join("pnpm-lock.yaml").exists(),
        "dedicated lockfiles must not write a shared workspace lockfile",
    );

    drop((root, mock_instance));
}

/// `pnpm --filter <project> unlink` strips the override and reinstalls only the
/// selected project — the unselected project's dedicated lockfile is never
/// created.
#[test]
fn filtered_unlink_with_dedicated_lockfiles_reinstalls_only_selected_project() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    const PKG: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
    let (first, second) = scaffold_dedicated_link_workspace(&workspace, PKG);

    let mut unlink = std::process::Command::cargo_bin("pnpm").expect("locate pacquet binary");
    unlink.current_dir(&workspace);
    unlink.with_args(["--filter", "first", "unlink", PKG]).assert().success();

    let workspace_yaml = read_workspace_yaml(&workspace);
    assert!(
        !workspace_yaml.contains("link:"),
        "the link override must be removed: {workspace_yaml}",
    );
    let first_lockfile =
        fs::read_to_string(first.join("pnpm-lock.yaml")).expect("selected project lockfile");
    assert!(
        first_lockfile.contains("dep-of-pkg-with-1-dep@100.0.0"),
        "the selected project must re-resolve from the registry: {first_lockfile}",
    );
    assert!(
        !second.join("pnpm-lock.yaml").exists(),
        "the unselected project must not be installed",
    );

    drop((root, mock_instance));
}
