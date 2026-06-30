//! End-to-end coverage for the `dedupeInjectedDeps` setting.
//!
//! Ports the spirit of pnpm's `'injected local packages are deduped'`
//! test at
//! [`installing/deps-installer/test/install/injectLocalPackages.ts:1785`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/test/install/injectLocalPackages.ts#L1785):
//! a workspace dep marked `dependenciesMeta[<alias>].injected: true`
//! whose target project has no transitive deps must collapse from
//! `file:<workspace>` back to `link:<rel>` once the dedupe pass
//! recognizes the children-subset case.

pub mod _utils;

use _utils::enable_gvs_in_workspace_yaml;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::is_symlink_or_junction,
};
use std::{fs, process::Command};

/// Two-project workspace where `a` injects leaf `b`. With the default
/// `dedupeInjectedDeps: true`, the install pass rewrites `a`'s direct
/// dep from `file:packages/b` back to `link:../b` because `b`'s
/// transitive deps are empty (vacuous subset).
#[test]
fn injected_leaf_workspace_dep_is_deduped_to_link() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "ws-root", "version": "0.0.0", "private": true }).to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/a")).expect("mkdir packages/a");
    fs::write(
        workspace.join("packages/a/package.json"),
        serde_json::json!({
            "name": "a",
            "version": "1.0.0",
            "dependencies": { "b": "workspace:*" },
            "dependenciesMeta": { "b": { "injected": true } },
        })
        .to_string(),
    )
    .expect("write packages/a/package.json");

    fs::create_dir_all(workspace.join("packages/b")).expect("mkdir packages/b");
    fs::write(
        workspace.join("packages/b/package.json"),
        serde_json::json!({ "name": "b", "version": "1.0.0" }).to_string(),
    )
    .expect("write packages/b/package.json");

    pacquet.with_arg("install").assert().success();

    let dep = workspace.join("packages/a/node_modules/b");
    assert!(
        is_symlink_or_junction(&dep).expect("query packages/a/node_modules/b"),
        "packages/a/node_modules/b should be a symlink — dedupeInjectedDeps was supposed to rewrite the injected file: dep back to link:",
    );

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("link:../b"),
        "pnpm-lock.yaml should record b as link:../b under packages/a:\n{lockfile}",
    );
    assert!(
        !lockfile.contains("file:packages/b"),
        "pnpm-lock.yaml should not retain the file:packages/b snapshot after dedupe:\n{lockfile}",
    );

    drop((root, mock_instance));
}

/// `injectWorkspacePackages: true` with a workspace dep (`b`) that has
/// its own dependency (`@pnpm.e2e/foo`), so the injected snapshot has a
/// non-empty child set. The dedupe pass must still collapse `a`'s
/// `file:packages/b` to `link:../b` through the children-subset branch
/// (not just the vacuous empty-children case the leaf test covers), and
/// the `link:` must survive a `remove` that re-resolves the whole
/// workspace. Behavioral analog of pnpm/pnpm#11448, whose single-project
/// `pnpm rm` regression switched such a dep from `link:` to `file:`.
/// Pacquet always re-resolves the full workspace (no single-project
/// mode), so the same resolve path backs both `install` and `remove`.
#[test]
fn injected_workspace_dep_with_children_stays_link_after_remove() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "ws-root",
            "version": "0.0.0",
            "private": true,
            "dependencies": { "@pnpm.e2e/bar": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\ninjectWorkspacePackages: true\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/a")).expect("mkdir packages/a");
    fs::write(
        workspace.join("packages/a/package.json"),
        serde_json::json!({
            "name": "a",
            "version": "1.0.0",
            "dependencies": { "b": "workspace:*" },
        })
        .to_string(),
    )
    .expect("write packages/a/package.json");

    fs::create_dir_all(workspace.join("packages/b")).expect("mkdir packages/b");
    fs::write(
        workspace.join("packages/b/package.json"),
        serde_json::json!({
            "name": "b",
            "version": "1.0.0",
            "dependencies": { "@pnpm.e2e/foo": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/b/package.json");

    pacquet.with_arg("install").assert().success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("link:../b"),
        "injectWorkspacePackages should dedupe b (which has its own dependency) to link:../b:\n{lockfile}",
    );
    assert!(
        !lockfile.contains("file:packages/b"),
        "an injected workspace dep with children must not stay file:packages/b after dedupe:\n{lockfile}",
    );
    assert!(
        lockfile.contains("@pnpm.e2e/foo"),
        "b's own dependency should be resolved, proving the injected snapshot has children:\n{lockfile}",
    );

    // Removing an unrelated root dependency re-resolves the whole
    // workspace; the injected-with-children dedupe must hold across it.
    Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(&workspace)
        .with_arg("remove")
        .with_arg("@pnpm.e2e/bar")
        .assert()
        .success();

    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml after remove");
    assert!(
        lockfile.contains("link:../b"),
        "b must stay link:../b after remove re-resolves the workspace:\n{lockfile}",
    );
    assert!(
        !lockfile.contains("file:packages/b"),
        "remove must not switch the injected workspace dep back to file:packages/b:\n{lockfile}",
    );

    drop((root, mock_instance));
}

/// `injectWorkspacePackages: true` where workspace dep `b` declares a
/// peer dependency (`@pnpm.e2e/foo`) that `a` provides. `b`'s injected
/// resolution then depends on `a`'s peer context, so its importer entry
/// is the peer-suffixed `file:packages/b(@pnpm.e2e/foo@100.0.0)` and
/// must *not* collapse to `link:../b` — a plain link would strip the
/// peer context and change the trust-relevant lockfile identity. The
/// peer-suffixed form must also survive a `remove` that re-resolves the
/// whole workspace. Behavioral analog of the peer-suffixed single-project
/// `pnpm rm` case in pnpm/pnpm#11448; the dedupe pass keeps such an entry
/// in `file:` form because its children (the resolved peer) are not a
/// subset of `b`'s own direct deps.
#[test]
fn injected_peer_suffixed_workspace_dep_stays_file_after_remove() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "ws-root",
            "version": "0.0.0",
            "private": true,
            "dependencies": { "@pnpm.e2e/bar": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str(
        "packages:\n  - 'packages/*'\ninjectWorkspacePackages: true\nautoInstallPeers: false\n",
    );
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/a")).expect("mkdir packages/a");
    fs::write(
        workspace.join("packages/a/package.json"),
        serde_json::json!({
            "name": "a",
            "version": "1.0.0",
            "dependencies": { "b": "workspace:*", "@pnpm.e2e/foo": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/a/package.json");

    fs::create_dir_all(workspace.join("packages/b")).expect("mkdir packages/b");
    fs::write(
        workspace.join("packages/b/package.json"),
        serde_json::json!({
            "name": "b",
            "version": "1.0.0",
            "peerDependencies": { "@pnpm.e2e/foo": ">=100.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/b/package.json");

    pacquet.with_arg("install").assert().success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("file:packages/b(@pnpm.e2e/foo@100.0.0)"),
        "a peer-resolved injected dep must stay in its peer-suffixed file: form, not collapse to a plain link::\n{lockfile}",
    );
    assert!(
        !lockfile.contains("link:../b"),
        "the peer-suffixed injected dep must not be deduped to link:../b — that would strip b's peer context:\n{lockfile}",
    );

    // Removing an unrelated root dependency re-resolves the whole
    // workspace; the peer-suffixed file: entry must survive intact.
    Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(&workspace)
        .with_arg("remove")
        .with_arg("@pnpm.e2e/bar")
        .assert()
        .success();

    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml after remove");
    assert!(
        lockfile.contains("file:packages/b(@pnpm.e2e/foo@100.0.0)"),
        "the peer-suffixed file: entry must be preserved byte-for-byte after remove:\n{lockfile}",
    );
    assert!(
        !lockfile.contains("link:../b"),
        "remove must not collapse the peer-suffixed injected dep to link:../b:\n{lockfile}",
    );

    drop((root, mock_instance));
}

/// Same fixture as the dedupe-true test but with
/// `dedupeInjectedDeps: false` in `pnpm-workspace.yaml`: the injected
/// snapshot stays as `file:packages/b` in the importer entry, the
/// lockfile writer's [`ImporterDepVersion::File`] arm formats it, and
/// `create_virtual_store` materialises the source project's contents
/// into the (escaped) `b@file+packages+b` virtual-store slot so the
/// per-importer symlink at `packages/a/node_modules/b` resolves to a
/// real directory. Guards both the lockfile-writer regression on
/// `file:` importer-level depPaths and the materialise step
/// (pnpm/pnpm#12038).
///
/// [`ImporterDepVersion::File`]: pacquet_lockfile::ImporterDepVersion::File
#[test]
fn injected_workspace_dep_with_dedupe_off_writes_file_arm() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "ws-root", "version": "0.0.0", "private": true }).to_string(),
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\ndedupeInjectedDeps: false\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    fs::create_dir_all(workspace.join("packages/a")).expect("mkdir packages/a");
    fs::write(
        workspace.join("packages/a/package.json"),
        serde_json::json!({
            "name": "a",
            "version": "1.0.0",
            "dependencies": { "b": "workspace:*" },
            "dependenciesMeta": { "b": { "injected": true } },
        })
        .to_string(),
    )
    .expect("write packages/a/package.json");

    fs::create_dir_all(workspace.join("packages/b")).expect("mkdir packages/b");
    fs::write(
        workspace.join("packages/b/package.json"),
        serde_json::json!({ "name": "b", "version": "1.0.0" }).to_string(),
    )
    .expect("write packages/b/package.json");

    pacquet.with_arg("install").assert().success();

    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("file:packages/b"),
        "pnpm-lock.yaml should retain the file:packages/b entry when dedupe is disabled:\n{lockfile}",
    );
    assert!(
        !lockfile.contains("link:../b"),
        "pnpm-lock.yaml should not rewrite the injected dep to link:../b when dedupe is disabled:\n{lockfile}",
    );

    let dep = workspace.join("packages/a/node_modules/b");
    assert!(
        is_symlink_or_junction(&dep).expect("query packages/a/node_modules/b"),
        "packages/a/node_modules/b should be a symlink into the virtual store when dedupe is disabled",
    );
    assert!(
        dep.join("package.json").is_file(),
        "packages/a/node_modules/b should resolve to a materialised slot — create_virtual_store must copy packages/b's contents into the file: snapshot's virtual-store directory (pnpm/pnpm#12038)",
    );

    drop((root, mock_instance));
}

/// Same `dedupeInjectedDeps: false` fixture, but with the global
/// virtual store enabled. The GVS slot path is
/// `<store>/links/<scope>/<name>/<version>/<hash>`, and the `<version>`
/// segment for a `file:` directory dep has no semver to fill it: pnpm's
/// `nameVerFromPkgSnapshot` yields `undefined`, which its
/// `formatGlobalVirtualStorePath` renders as the literal `undefined`
/// segment. Pacquet must do the same — emitting the raw `file:packages/b`
/// version would put a `:` (and an embedded `/`) into the slot path,
/// which Windows rejects with `ERROR_INVALID_NAME`. Regression test for
/// pnpm/pnpm#12038.
#[test]
fn injected_workspace_dep_with_dedupe_off_materialises_under_gvs() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "ws-root", "version": "0.0.0", "private": true }).to_string(),
    )
    .expect("write root package.json");

    enable_gvs_in_workspace_yaml(
        &workspace,
        "packages:\n  - 'packages/*'\ndedupeInjectedDeps: false\n",
    );

    fs::create_dir_all(workspace.join("packages/a")).expect("mkdir packages/a");
    fs::write(
        workspace.join("packages/a/package.json"),
        serde_json::json!({
            "name": "a",
            "version": "1.0.0",
            "dependencies": { "b": "workspace:*" },
            "dependenciesMeta": { "b": { "injected": true } },
        })
        .to_string(),
    )
    .expect("write packages/a/package.json");

    fs::create_dir_all(workspace.join("packages/b")).expect("mkdir packages/b");
    fs::write(
        workspace.join("packages/b/package.json"),
        serde_json::json!({ "name": "b", "version": "1.0.0" }).to_string(),
    )
    .expect("write packages/b/package.json");

    pacquet.with_arg("install").assert().success();

    let dep = workspace.join("packages/a/node_modules/b");
    assert!(
        is_symlink_or_junction(&dep).expect("query packages/a/node_modules/b"),
        "packages/a/node_modules/b should be a symlink into the global virtual store",
    );
    assert!(
        dep.join("package.json").is_file(),
        "packages/a/node_modules/b should resolve to a materialised GVS slot (pnpm/pnpm#12038)",
    );

    // The slot path must not embed the raw `file:` version — that is
    // the `ERROR_INVALID_NAME` shape on Windows the fix removes.
    #[cfg(unix)]
    {
        let target = fs::read_link(&dep).expect("read packages/a/node_modules/b symlink");
        assert!(
            !target.to_string_lossy().contains("file:"),
            "GVS slot path must not contain a `file:` segment with a colon; got {target:?}",
        );
    }

    drop((root, mock_instance));
}
