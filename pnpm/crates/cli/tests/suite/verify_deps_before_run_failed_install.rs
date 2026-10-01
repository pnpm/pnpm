//! The `install` action of the `verify-deps-before-run` gate when the
//! install it spawns fails
//! ([pnpm/pnpm#15173](https://github.com/pnpm/pnpm/issues/15173)).

use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::fs;

/// A sandbox with a read-only store or no network cannot install. The
/// failed install is reported as a warning and the script still runs.
#[test]
fn failed_install_warns_and_runs_the_script() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let marker = workspace.join("marker.txt");
    let manifest = json!({
        "name": "verify-deps-project",
        "version": "0.0.0",
        "dependencies": {
            "@pnpm.e2e/this-package-does-not-exist": "1.0.0",
        },
        "scripts": {
            "hello": r#"node -e "require('fs').writeFileSync('marker.txt', '')""#,
        },
    });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");

    let output = pacquet
        .with_args(["run", "hello"])
        .output()
        .expect("spawn pacquet run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "a failed install must not block the script:\n{stderr}");
    assert!(
        stderr.contains(
            r#"The install that runs before scripts failed, so your node_modules may be out of sync with your lockfile. Set "verifyDepsBeforeRun: false" to skip this install."#
        ),
        "expected the failed-install warning:\n{stderr}",
    );
    assert!(marker.exists(), "the script must run");

    drop(root);
}
