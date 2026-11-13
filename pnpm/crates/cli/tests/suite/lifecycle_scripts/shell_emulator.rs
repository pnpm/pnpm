use super::{
    _utils,
    workspace_yaml::{allow_builds, append_workspace_yaml_key},
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

fn enable_shell_emulator(workspace: &Path) {
    append_workspace_yaml_key(workspace, "shellEmulator", true);
}

#[test]
fn runs_the_projects_own_scripts_and_dev_preinstall() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "name": "project-under-the-shell-emulator",
        "version": "1.0.0",
        "scripts": {
            "pnpm:devPreinstall": "echo ran > dev-preinstall.txt",
            "postinstall": "echo ran > postinstall.txt",
        },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    enable_shell_emulator(&workspace);

    pacquet
        .with_arg("install")
        .assert()
        .success();

    assert!(workspace.join("dev-preinstall.txt").exists(), "pnpm:devPreinstall must run");
    assert!(workspace.join("postinstall.txt").exists(), "the postinstall must run");

    drop((root, mock_instance));
}

#[test]
fn runs_dependency_build_scripts() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "dependencies": { "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    allow_builds(&workspace, &[("@pnpm.e2e/pre-and-postinstall-scripts-example", true)]);
    enable_shell_emulator(&workspace);

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let pkg_dir = workspace.join(
        "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
             /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example",
    );
    assert!(pkg_dir.join("generated-by-preinstall.js").exists());
    assert!(pkg_dir.join("generated-by-postinstall.js").exists());

    drop((root, mock_instance));
}

#[test]
fn expands_braced_parameter_expansions_in_scripts() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let package_json = serde_json::json!({
        "name": "project-reading-a-braced-parameter",
        "version": "1.0.0",
        "scripts": {
            "write-var": "node -e \"require('fs').writeFileSync('out.txt', process.argv[1])\" \
                pre${MY_VAR:-fallback}post",
        },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    enable_shell_emulator(&workspace);

    pacquet
        .with_arg("run")
        .with_arg("write-var")
        .with_env("MY_VAR", "hello")
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(workspace.join("out.txt")).expect("read out.txt"),
        "prehellopost",
    );

    // The fallback is only reached when the variable is unset, so the host's
    // own environment must not decide it.
    let mut without_the_var = _utils::pacquet_in(&workspace);
    without_the_var.env_remove("MY_VAR");
    without_the_var
        .with_arg("run")
        .with_arg("write-var")
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(workspace.join("out.txt")).expect("read out.txt"),
        "prefallbackpost",
    );

    drop((root, mock_instance));
}

#[test]
fn preserves_quoting_after_escaped_backslashes_and_single_quotes() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("record.cjs"),
        "require('node:fs').writeFileSync('args.json', JSON.stringify(process.argv.slice(2)))",
    )
    .expect("write record.cjs");
    let package_json = serde_json::json!({
        "name": "project-reading-escaped-backslashes",
        "version": "1.0.0",
        "scripts": {
            "repro": r#"node record.cjs "a\\" '${UNSET_VAR:-expanded_by_mistake}' "dist\\${OUT_DIR:-app}" \' ${OUT_DIR:-app}"#,
        },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    enable_shell_emulator(&workspace);

    let mut command = pacquet;
    command.env_remove("UNSET_VAR");
    command.env_remove("OUT_DIR");
    command
        .with_args(["run", "repro"])
        .assert()
        .success();

    let recorded: Vec<String> =
        serde_json::from_slice(&fs::read(workspace.join("args.json")).expect("read args.json"))
            .expect("parse args.json");
    assert_eq!(recorded, vec![r"a\", "${UNSET_VAR:-expanded_by_mistake}", r"dist\app", "'", "app"]);

    drop((root, mock_instance));
}
