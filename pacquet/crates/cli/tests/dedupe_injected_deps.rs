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
use std::fs;

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
