use super::{fs_remove_dir_all, pacquet_at, write_manifest, write_workspace_yaml};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::process::Command;

#[test]
fn identical_transitive_peers_share_one_copy_under_a_conflicting_host() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_manifest(
        &workspace,
        serde_json::json!({
            "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0",
            "@pnpm.e2e/has-has-y-peer-only-as-peer-and-y": "1.0.0",
            "@pnpm/y": "2.0.0",
        }),
    );
    write_workspace_yaml(
        &workspace,
        "nodeLinker: hoisted\nautoInstallPeers: false\npackageExtensions:\n  '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0':\n    dependencies:\n      '@pnpm.e2e/has-y-peer': 1.0.0\n  '@pnpm.e2e/has-has-y-peer-only-as-peer-and-y@1.0.0':\n    peerDependencies:\n      '@pnpm.e2e/dep-of-pkg-with-1-dep': 100.0.0\n",
    );
    let assert_layout = |install: &str| {
        let output = Command::new("node")
            .arg("-e")
            .arg(r"
const assert = require('node:assert/strict');
const path = require('node:path');
const resolve = (name, from = process.cwd()) => require.resolve(`${name}/package.json`, { paths: [from] });
const host = path.dirname(resolve('@pnpm.e2e/has-has-y-peer-only-as-peer-and-y'));
const client = resolve('@pnpm.e2e/dep-of-pkg-with-1-dep', host);
assert.equal(client, resolve('@pnpm.e2e/dep-of-pkg-with-1-dep'));
const bridge = path.dirname(resolve('@pnpm.e2e/has-y-peer', path.dirname(client)));
assert.equal(require(resolve('@pnpm/y', bridge)).version, '2.0.0');
assert.equal(require(resolve('@pnpm/y', host)).version, '1.0.0');
")
            .current_dir(&workspace)
            .output()
            .expect("run Node.js resolution assertions");
        assert!(output.status.success(), "{install}: {output:?}");
    };

    pacquet
        .with_args(["install"])
        .assert()
        .success();
    assert_layout("fresh install");
    fs_remove_dir_all(&workspace.join("node_modules"));
    pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_layout("frozen install");
    drop((root, mock_instance));
}
