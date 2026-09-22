//! The interactive `minimumReleaseAge` approval prompt.
//!
//! Unix-only by harness: driving the prompt needs a pseudo-terminal, and
//! the fixture opens one with Python's `pty`, which exists only on Unix.
//! Covering Windows means a different way to allocate a console, not a
//! port of this file.
#![cfg(unix)]

use crate::_utils::{
    append_workspace_yaml_key,
    set_minimum_release_age,
    without_colors,
};
use pnpm_config::WorkspaceSettings;
use pnpm_testing_utils::{
    bin::CommandTempCwd,
    command_env::CommandTestExt,
};
use std::{
    fs,
    process::Command,
};

#[test]
fn approval_prints_the_version_list_once_and_persists_excludes() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    fs::write(workspace.join("package.json"), r#"{"name":"approval-test","version":"1.0.0"}"#)
        .expect("write package.json");
    set_minimum_release_age(&workspace, 60 * 24 * 365 * 100);
    append_workspace_yaml_key(&workspace, "minimumReleaseAgeStrict", true);

    let output = without_colors(Command::new("python3").without_ambient_pnpm_config())
        .env("CI", "false")
        .env_remove("GITHUB_ACTION")
        .arg("-c")
        .arg(include_str!("../fixtures/minimum_release_age_prompt.py"))
        .arg(pacquet.get_program())
        .args([
            "add",
            "@pnpm.e2e/bravo-dep@1.0.0",
            "@pnpm.e2e/hello-world-js-bin@1.0.0",
            "--reporter=append-only",
            "--ignore-scripts",
        ])
        .current_dir(&workspace)
        .output()
        .expect("run interactive install in a pseudo-terminal");
    let stdout = String::from_utf8(output.stdout).expect("terminal output is UTF-8");
    eprintln!("{stdout}");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        stdout.matches("2 versions do not meet the minimumReleaseAge constraint:").count(),
        1,
    );

    let question =
        "Add to minimumReleaseAgeExclude in pnpm-workspace.yaml and proceed with the install?";
    let prompt_output = &stdout[..stdout.rfind(question).expect("approval question is rendered")];
    let versions = ["@pnpm.e2e/bravo-dep@1.0.0", "@pnpm.e2e/hello-world-js-bin@1.0.0"];
    for version in versions {
        assert_eq!(
            prompt_output
                .lines()
                .filter(|line| line.trim() == version)
                .count(),
            1,
            "{version}",
        );
    }
    let settings = WorkspaceSettings::load_at(&workspace)
        .expect("read workspace manifest")
        .expect("workspace manifest exists");
    assert_eq!(settings.minimum_release_age_exclude.unwrap(), versions);

    drop((root, npmrc_info));
}
