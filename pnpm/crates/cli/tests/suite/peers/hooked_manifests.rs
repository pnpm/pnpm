use super::{PEERS_CHECK_HINT, write_linked_peer_workspace};
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::fs;

/// `pnpm peers check` reads the linked project's `package.json`, so the
/// install's report must not see the `readPackage` rewrite of it either.
#[test]
fn a_read_package_hook_does_not_settle_a_linked_projects_declared_peer() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_linked_peer_workspace(&workspace);
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"module.exports = { hooks: { readPackage (pkg) {
  if (pkg.name === 'lib') pkg.peerDependencies['@pnpm.e2e/foo'] = '2.0.0'
  return pkg
} } }
",
    )
    .expect("write .pnpmfile.cjs");

    let output = pacquet
        .with_args(["install", "--lockfile-only"])
        .output()
        .expect("run pnpm install");
    assert!(output.status.success(), "install must succeed: {output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains(PEERS_CHECK_HINT), "stdout:\n{stdout}");

    drop((root, mock_instance));
}
