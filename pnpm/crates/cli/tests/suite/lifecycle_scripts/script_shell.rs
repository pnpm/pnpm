use super::workspace_yaml::{allow_builds, append_workspace_yaml_key};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

/// Install a shell shim that appends each script it is asked to run
/// to a log beside itself, then hands the script to the real
/// `/bin/sh`, and point `scriptShell` at it. Every spawn that honors
/// the setting shows up in the log; every spawn that ignores it
/// silently does not.
///
/// The shim derives the log path from its own location rather than
/// having it interpolated in, so nothing from the temp directory's
/// name is ever parsed as shell source.
fn install_probe_shell(workspace: &Path) -> std::path::PathBuf {
    let log = workspace.join("shell-invocations.txt");
    let shim = workspace.join("probe-shell.sh");
    fs::write(
            &shim,
            "#!/bin/sh\nprintf '%s\\n' \"$2\" >> \"$(dirname \"$0\")/shell-invocations.txt\"\nexec /bin/sh -c \"$2\"\n",
        )
        .expect("write the probe shell");
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755))
        .expect("make the probe shell executable");
    append_workspace_yaml_key(workspace, "scriptShell", shim.display());
    log
}

fn logged_scripts(log: &Path) -> Vec<String> {
    fs::read_to_string(log)
        .expect("read the probe shell's log")
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn runs_the_projects_own_scripts_and_dev_preinstall_under_it() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "name": "project-with-a-configured-shell",
        "version": "1.0.0",
        "scripts": {
            "pnpm:devPreinstall": "echo dev-preinstall-marker",
            "postinstall": "echo postinstall-marker",
        },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    let log = install_probe_shell(&workspace);

    pacquet.with_arg("install").assert().success();

    let scripts = logged_scripts(&log);
    assert!(
        scripts.iter().any(|script| script.contains("dev-preinstall-marker")),
        "pnpm:devPreinstall should run under the configured shell, got {scripts:?}",
    );
    assert!(
        scripts.iter().any(|script| script.contains("postinstall-marker")),
        "the project's postinstall should run under the configured shell, got {scripts:?}",
    );

    drop((root, mock_instance));
}

#[test]
fn runs_dependency_build_scripts_under_it() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    allow_builds(&workspace, &[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]);
    let log = install_probe_shell(&workspace);

    pacquet.with_arg("install").assert().success();

    let scripts = logged_scripts(&log);
    assert!(
        scripts.iter().any(|script| script.contains("generated-by-preinstall")),
        "a dependency's build scripts should run under the configured shell, got {scripts:?}",
    );

    drop((root, mock_instance));
}
