//! End-to-end coverage for the `dedupeInjectedDeps` setting.
//!
//! Ports the spirit of pnpm's `'injected local packages are deduped'`
//! test at
//! [`installing/deps-installer/test/install/injectLocalPackages.ts:1785`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/test/install/injectLocalPackages.ts#L1785):
//! a workspace dep marked `dependenciesMeta[<alias>].injected: true`
//! whose target project has no transitive deps must collapse from
//! `file:<workspace>` back to `link:<rel>` once the dedupe pass
//! recognizes the children-subset case.

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
/// the on-disk layout points at the virtual store slot instead of a
/// `link:` sibling. Regression test for the writer panic on `file:`
/// importer-level depPaths that the resolver-side dedupe used to hide.
///
/// Windows-skipped: pacquet's `create_virtual_store` pass does not
/// materialise `file:<workspace>` snapshots into the virtual store
/// yet (broader gap tracked under pnpm/pnpm#12009's
/// `injectWorkspacePackages` line), so the symlink at
/// `packages/a/node_modules/b` points at a non-existent target. Unix
/// silently tolerates the broken link during the bin-link manifest
/// walk; Windows is stricter and trips `ERROR_INVALID_NAME` reading
/// through it with a mixed-separator path. The lockfile-writer
/// regression this test guards is platform-independent, so the
/// Linux + macOS coverage is enough until the materialise gap closes.
///
/// [`ImporterDepVersion::File`]: pacquet_lockfile::ImporterDepVersion::File
#[test]
#[cfg_attr(target_os = "windows", ignore = "file:<workspace> materialisation not ported")]
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

    drop((root, mock_instance));
}
