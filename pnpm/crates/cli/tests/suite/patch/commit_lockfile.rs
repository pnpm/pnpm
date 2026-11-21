//! How `pnpm patch-commit` updates the lockfile snapshots of the package it
//! patched.

use super::{
    AddMockedRegistry,
    CommandTempCwd,
    append_workspace_yaml_key,
    fs,
    pacquet,
    setup_installed,
    write_patch_edit,
};
use assert_cmd::assert::OutputAssertExt;
use tempfile::TempDir;

#[test]
fn patch_commit_updates_lockfile_when_manifest_dependencies_removed() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet(&workspace, ["install", "--reporter=silent"]).assert().success();

    let lockfile_text =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        lockfile_text.contains("@pnpm.e2e/dep-of-pkg-with-1-dep"),
        "initial lockfile should contain dep-of-pkg-with-1-dep",
    );

    pacquet(&workspace, ["patch", "@pnpm.e2e/pkg-with-1-dep@100.0.0", "--reporter=silent"])
        .assert()
        .success();

    let edit_dir = workspace.join("node_modules/.pnpm_patches/@pnpm.e2e/pkg-with-1-dep@100.0.0");
    let edit_manifest_path = edit_dir.join("package.json");
    let mut manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&edit_manifest_path).expect("read edit package.json"),
    )
    .expect("parse edit package.json");
    manifest
        .as_object_mut()
        .unwrap()
        .remove("dependencies");
    fs::write(&edit_manifest_path, serde_json::to_string_pretty(&manifest).unwrap())
        .expect("write edit package.json");

    pacquet(
        &workspace,
        ["patch-commit", edit_dir.to_str().expect("utf8 edit dir"), "--reporter=silent"],
    )
    .assert()
    .success();

    let updated_lockfile_text =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read updated lockfile");
    assert!(
        !updated_lockfile_text.contains("@pnpm.e2e/dep-of-pkg-with-1-dep"),
        "dep-of-pkg-with-1-dep should be pruned from lockfile: {updated_lockfile_text}",
    );

    drop((root, mock_instance));
}

#[test]
fn patch_commit_preserves_resolved_peer_dependencies_in_snapshot() {
    let (root, workspace, npmrc_info) = setup_installed();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/wants-peer-c-1": "1.0.0",
                "@pnpm.e2e/peer-c": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet(&workspace, ["install", "--reporter=silent"]).assert().success();

    let initial_lockfile_text =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read initial lockfile");
    assert!(
        initial_lockfile_text.contains("@pnpm.e2e/peer-c"),
        "initial lockfile should contain peer-c",
    );

    pacquet(&workspace, ["patch", "@pnpm.e2e/wants-peer-c-1@1.0.0", "--reporter=silent"])
        .assert()
        .success();

    let edit_dir = workspace.join("node_modules/.pnpm_patches/@pnpm.e2e/wants-peer-c-1@1.0.0");
    write_patch_edit(&edit_dir, "patched wants peer");

    pacquet(
        &workspace,
        ["patch-commit", edit_dir.to_str().expect("utf8 edit dir"), "--reporter=silent"],
    )
    .assert()
    .success();

    let updated_lockfile =
        pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace).unwrap().unwrap();
    let peer_c: pnpm_lockfile::PkgName = "@pnpm.e2e/peer-c".parse().unwrap();
    let wants_peer = updated_lockfile.snapshots
        .as_ref()
        .expect("snapshots must exist")
        .iter()
        .find(|(key, _)| key.name.to_string() == "@pnpm.e2e/wants-peer-c-1")
        .map(|(_, snapshot)| snapshot)
        .expect("wants-peer-c-1 snapshot must exist");
    assert!(
        wants_peer.dependencies
            .as_ref()
            .is_some_and(|deps| deps.contains_key(&peer_c)),
        "resolved peer dependency @pnpm.e2e/peer-c should remain in snapshot dependencies: {wants_peer:#?}",
    );

    drop((root, mock_instance));
}

/// Installs a package bound to an optional peer under the next lockfile
/// format, which refuses to save a snapshot whose edges disagree with its key.
fn setup_optional_peer_in_next_lockfile() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    append_workspace_yaml_key(&workspace, "lockfile", "\n  format: v13");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/abc-optional-peers": "1.0.0",
                "@pnpm.e2e/peer-a": "1.0.0",
                "@pnpm.e2e/peer-b": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    pacquet(&workspace, ["install", "--reporter=silent"]).assert().success();
    pacquet(&workspace, ["patch", "@pnpm.e2e/abc-optional-peers@1.0.0", "--reporter=silent"])
        .assert()
        .success();
    (root, workspace, npmrc_info)
}

fn commit_optional_peer_patch_and_assert_peer_kept(workspace: &std::path::Path) {
    let edit_dir = workspace.join("node_modules/.pnpm_patches/@pnpm.e2e/abc-optional-peers@1.0.0");
    pacquet(
        workspace,
        ["patch-commit", edit_dir.to_str().expect("utf8 edit dir"), "--reporter=silent"],
    )
    .assert()
    .success();

    let lockfile = pnpm_lockfile::Lockfile::load_wanted_from_dir(workspace).unwrap().unwrap();
    let peer_b: pnpm_lockfile::PkgName = "@pnpm.e2e/peer-b".parse().unwrap();
    let (key, snapshot) = lockfile.snapshots
        .as_ref()
        .expect("snapshots must exist")
        .iter()
        .find(|(key, _)| key.name.to_string() == "@pnpm.e2e/abc-optional-peers")
        .expect("abc-optional-peers snapshot must exist");
    assert!(
        snapshot.optional_dependencies
            .as_ref()
            .is_some_and(|deps| deps.contains_key(&peer_b)),
        "bound optional peer should remain in {key}: {snapshot:#?}",
    );
    pacquet(workspace, ["install", "--frozen-lockfile", "--reporter=silent"]).assert().success();
}

#[test]
fn patch_commit_keeps_bound_optional_peers_in_next_lockfile() {
    let (root, workspace, npmrc_info) = setup_optional_peer_in_next_lockfile();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let edit_dir = workspace.join("node_modules/.pnpm_patches/@pnpm.e2e/abc-optional-peers@1.0.0");
    write_patch_edit(&edit_dir, "patched optional peers");

    commit_optional_peer_patch_and_assert_peer_kept(&workspace);

    drop((root, mock_instance));
}

/// Peers are resolved from the registry's manifest, so a patch that drops a
/// peer from `package.json` leaves its binding in place.
#[test]
fn patch_commit_keeps_a_bound_peer_the_patch_removes() {
    let (root, workspace, npmrc_info) = setup_optional_peer_in_next_lockfile();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let edit_manifest_path = workspace.join(
        "node_modules/.pnpm_patches/@pnpm.e2e/abc-optional-peers@1.0.0/package.json",
    );
    fs::write(
        &edit_manifest_path,
        serde_json::json!({
            "name": "@pnpm.e2e/abc-optional-peers",
            "version": "1.0.0",
        })
        .to_string(),
    )
    .expect("write edit package.json");

    commit_optional_peer_patch_and_assert_peer_kept(&workspace);

    drop((root, mock_instance));
}

/// Installs `dependencies` with the `work:` named-registry alias pointing at
/// the mocked registry, which the lockfile keys as
/// `<name>@<registryName>:<version>`.
fn setup_named_registry_install(
    dependencies: &serde_json::Value,
) -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let registry = npmrc_info.mock_instance.url();
    append_workspace_yaml_key(&workspace, "namedRegistries", format!("{{ work: '{registry}' }}"));
    append_workspace_yaml_key(&workspace, "registry", registry.replace("127.0.0.1", "localhost"));
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": dependencies }).to_string(),
    )
    .expect("write package.json");
    pacquet(&workspace, ["install", "--reporter=silent"]).assert().success();
    (root, workspace, npmrc_info)
}

#[test]
fn patch_and_patch_commit_reach_a_named_registry_package() {
    let (root, workspace, npmrc_info) =
        setup_named_registry_install(&serde_json::json!({ "is-positive": "work:1.0.0" }));
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "is-positive@1.0.0", "--reporter=silent"]).assert().success();
    let edit_dir = workspace.join("node_modules/.pnpm_patches/is-positive@1.0.0");
    write_patch_edit(&edit_dir, "patched named registry");
    pacquet(
        &workspace,
        ["patch-commit", edit_dir.to_str().expect("utf8 edit dir"), "--reporter=silent"],
    )
    .assert()
    .success();

    assert!(workspace.join("patches/is-positive@1.0.0.patch").exists());
    let installed = fs::read_to_string(workspace.join("node_modules/is-positive/index.js"))
        .expect("read installed index.js");
    assert!(installed.contains("patched named registry"), "index.js: {installed}");
    pacquet(&workspace, ["install", "--frozen-lockfile", "--reporter=silent"]).assert().success();

    drop((root, mock_instance));
}

#[test]
fn patch_commit_prunes_a_dependency_removed_from_a_named_registry_package() {
    let (root, workspace, npmrc_info) = setup_named_registry_install(
        &serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "work:100.0.0" }),
    );
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    pacquet(&workspace, ["patch", "@pnpm.e2e/pkg-with-1-dep@100.0.0", "--reporter=silent"])
        .assert()
        .success();
    let edit_dir = workspace.join("node_modules/.pnpm_patches/@pnpm.e2e/pkg-with-1-dep@100.0.0");
    fs::write(
        edit_dir.join("package.json"),
        serde_json::json!({ "name": "@pnpm.e2e/pkg-with-1-dep", "version": "100.0.0" }).to_string(),
    )
    .expect("write edit package.json");
    pacquet(
        &workspace,
        ["patch-commit", edit_dir.to_str().expect("utf8 edit dir"), "--reporter=silent"],
    )
    .assert()
    .success();

    let lockfile_text =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read updated lockfile");
    assert!(
        !lockfile_text.contains("@pnpm.e2e/dep-of-pkg-with-1-dep"),
        "dep-of-pkg-with-1-dep should be pruned from lockfile: {lockfile_text}",
    );

    drop((root, mock_instance));
}
