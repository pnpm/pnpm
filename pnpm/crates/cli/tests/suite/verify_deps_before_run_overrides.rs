//! The `verify-deps-before-run` gate with `overrides` that point a
//! dependency at a workspace project.

use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{
    AddMockedRegistry,
    CommandTempCwd,
};
use serde_json::json;
use std::fs;

/// Under dedicated lockfiles with `injectWorkspacePackages`, a dependency an
/// override turns into `link:` is a symlink, as a declared `link:` is, so its
/// target project must be installed.
#[test]
fn filtered_exec_holds_a_link_override_under_dedicated_lockfiles_with_injection() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "workspace-root", "version": "0.0.0" }).to_string(),
    )
    .expect("write root package.json");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "sharedWorkspaceLockfile: false\ninjectWorkspacePackages: true\npackages:\n  - packages/*\noverrides:\n  baz: link:./packages/baz\n",
    )
    .expect("write pnpm-workspace.yaml");
    let projects =
        [("foo", json!({ "baz": "^1.0.0" })), ("baz", json!({ "@pnpm.e2e/foo": "100.0.0" }))];
    for (name, dependencies) in projects {
        let project = workspace.join("packages").join(name);
        fs::create_dir_all(&project).expect("create workspace project");
        let manifest = json!({ "name": name, "version": "1.0.0", "dependencies": dependencies });
        fs::write(project.join("package.json"), manifest.to_string()).expect("write package.json");
    }

    pacquet_in(&workspace)
        .with_args(["--filter", "foo", "install"])
        .assert()
        .success();
    assert!(!workspace.join("packages/baz/node_modules").exists());

    let exec = |action: &str, script: &str| {
        pacquet_in(&workspace)
            .with_args([
                &format!("--config.verify-deps-before-run={action}"),
                "--filter",
                "foo",
                "exec",
                "node",
                "-e",
                script,
            ])
            .output()
            .expect("spawn pacquet exec")
    };

    let output = exec("error", "0");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success() && stderr.contains("ERR_PNPM_VERIFY_DEPS_BEFORE_RUN"),
        "a never-installed link: override target must fail the verify-deps check:\n{stderr}",
    );

    let output = exec("install", r#"process.stdout.write("linked-ok")"#);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the filtered exec must install the link: override target:\n{stderr}",
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "linked-ok");
    assert!(
        workspace.join("packages/baz/node_modules/@pnpm.e2e/foo").exists(),
        "the gate must install the link: override target:\n{stderr}",
    );

    let output = exec("error", "0");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

    drop((root, mock_instance));
}
