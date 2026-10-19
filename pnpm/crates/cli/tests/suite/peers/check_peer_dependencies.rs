use super::{PEERS_CHECK_HINT, write_peer_conflict_manifest};
use crate::_utils::{append_workspace_yaml_key, pacquet_in};
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::fs;

/// `checkPeerDependencies: false` drops the report of a resolving install
/// without changing what it resolves, and leaves the issues to
/// `pnpm peers check`.
#[test]
fn an_install_without_the_peer_check_leaves_the_issues_to_peers_check() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_peer_conflict_manifest(&workspace, "peer-conflict");
    let lockfile_path = workspace.join("pnpm-lock.yaml");

    let checked = pacquet
        .with_args(["install", "--lockfile-only"])
        .output()
        .expect("run pnpm install --lockfile-only");
    let stdout = String::from_utf8(checked.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains(PEERS_CHECK_HINT), "stdout:\n{stdout}");
    let checked_lockfile = fs::read_to_string(&lockfile_path).expect("read the lockfile");
    fs::remove_file(&lockfile_path).expect("remove the lockfile so the install resolves again");
    append_workspace_yaml_key(&workspace, "checkPeerDependencies", false);

    let unchecked = pacquet_in(&workspace)
        .with_args(["install", "--lockfile-only"])
        .output()
        .expect("run pnpm install --lockfile-only");
    assert!(unchecked.status.success(), "lockfile-only install must succeed: {unchecked:?}");
    let stdout = String::from_utf8(unchecked.stdout).expect("stdout is UTF-8");
    assert!(!stdout.contains(PEERS_CHECK_HINT), "stdout:\n{stdout}");
    assert_eq!(fs::read_to_string(&lockfile_path).expect("read the lockfile"), checked_lockfile);

    let peers = pacquet_in(&workspace)
        .with_args(["peers", "check"])
        .output()
        .expect("run pnpm peers check");
    assert_eq!(peers.status.code(), Some(1), "peers check still reports them: {peers:?}");

    drop((root, mock_instance));
}

/// Skipping the check would drop the failure `strictPeerDependencies`
/// asks for, so the install refuses the combination before it resolves.
#[test]
fn strict_peer_dependencies_rejects_an_install_without_the_peer_check() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_peer_conflict_manifest(&workspace, "peer-conflict");
    append_workspace_yaml_key(&workspace, "strictPeerDependencies", true);

    let output = pacquet
        .with_args(["install", "--config.check-peer-dependencies=false"])
        .output()
        .expect("run pnpm install");
    assert!(!output.status.success(), "install must fail: {output:?}");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.contains("ERR_PNPM_CONFIG_CONFLICT_STRICT_PEER_DEPENDENCIES_WITH_NO_PEER_CHECK"),
        "stderr:\n{stderr}",
    );
    assert!(!workspace.join("pnpm-lock.yaml").exists(), "the install must stop before resolving");

    drop((root, mock_instance));
}
