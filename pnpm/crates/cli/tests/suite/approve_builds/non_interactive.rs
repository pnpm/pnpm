#[cfg(unix)]
use super::pacquet_in_terminal;
use super::{
    INSTALL_MARKER,
    install_with_ignored_build,
    pacquet,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::fs;

#[test]
fn approve_builds_without_args_fails_when_stdin_is_not_a_terminal() {
    let (harness, workspace) = install_with_ignored_build();

    let assert = pacquet(&workspace)
        .with_env("CI", "false")
        .with_arg("approve-builds")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_APPROVE_BUILDS_NON_INTERACTIVE"), "stderr:\n{stderr}");
    assert!(!workspace.join(INSTALL_MARKER).exists());

    pacquet(&workspace)
        .with_env("CI", "true")
        .with_args(["approve-builds", "--all"])
        .assert()
        .success();
    assert!(workspace.join(INSTALL_MARKER).exists());

    pacquet(&workspace)
        .with_env("CI", "true")
        .with_arg("approve-builds")
        .assert()
        .success();

    drop(harness);
}

#[cfg(unix)]
#[test]
fn approve_builds_and_approve_reject_ci_even_with_a_controlling_terminal() {
    let (harness, workspace) = install_with_ignored_build();

    for cmd in ["approve-builds", "approve"] {
        let assert = pacquet_in_terminal(&workspace)
            .with_env("CI", "true")
            .with_arg(cmd)
            .assert()
            .failure();
        let output = String::from_utf8_lossy(&assert.get_output().stdout);
        let expected_code = if cmd == "approve" {
            "ERR_PNPM_APPROVE_NON_INTERACTIVE"
        } else {
            "ERR_PNPM_APPROVE_BUILDS_NON_INTERACTIVE"
        };
        assert!(output.contains(expected_code), "{cmd} output:\n{output}");
    }
    assert!(!workspace.join(INSTALL_MARKER).exists());

    pacquet_in_terminal(&workspace)
        .with_env("CI", "true")
        .with_args(["approve", "--all"])
        .assert()
        .success();
    assert!(workspace.join(INSTALL_MARKER).exists());

    pacquet_in_terminal(&workspace)
        .with_env("CI", "true")
        .with_arg("approve")
        .assert()
        .success();

    drop(harness);
}

#[test]
fn an_unmatched_grant_requires_a_terminal_before_any_decisions_are_written() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), r#"{"name":"app","version":"1.0.0"}"#)
        .expect("write package.json");
    fs::write(workspace.join("pnpm-workspace.yaml"), "").expect("write pnpm-workspace.yaml");

    pacquet(&workspace)
        .with_env("CI", "true")
        .with_arg("approve")
        .assert()
        .success();

    let result = pacquet(&workspace)
        .with_env("CI", "true")
        .with_args(["approve", "example", "!other-example", "-y"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&result.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_APPROVE_NON_INTERACTIVE"), "{stderr}");
    assert_eq!(fs::read(workspace.join("pnpm-workspace.yaml")).unwrap(), b"");

    drop(root);
}

#[cfg(unix)]
#[test]
fn dlx_skips_automatic_build_approval_prompt_in_ci_even_with_a_terminal() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let cmd = pacquet_in_terminal(&workspace).without_env("PNPM_AUTO_APPROVE_BUILDS_FOR_TESTS");
    cmd.with_env("CI", "true")
        .with_env("PNPM_CONFIG_STRICT_DEP_BUILDS", "false")
        .with_env("PNPM_CONFIG_STORE_DIR", &npmrc_info.store_dir)
        .with_env("PNPM_CONFIG_CACHE_DIR", &npmrc_info.cache_dir)
        .with_args(["dlx", "@pnpm.e2e/has-bin-and-needs-build"])
        .assert()
        .success();

    let cache_entry = fs::read_dir(npmrc_info.cache_dir.join("dlx"))
        .expect("read dlx cache")
        .next()
        .expect("dlx cache entry exists")
        .expect("read dlx cache entry")
        .path()
        .join("pkg");
    let artifact = cache_entry.join(
        "node_modules/.pnpm/@pnpm.e2e+install-script-example@1.0.0/node_modules/@pnpm.e2e/install-script-example/generated-by-install.js",
    );
    assert!(!artifact.exists(), "CI run must leave pending builds unexecuted");
    let cache_yaml =
        fs::read_to_string(cache_entry.join("pnpm-workspace.yaml")).unwrap_or_default();
    assert!(!cache_yaml.contains("allowBuilds:"), "cache yaml:\n{cache_yaml}");

    drop(root);
}

#[cfg(unix)]
#[test]
fn approve_selects_advance_permissions_without_changing_unselected_policy() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("package.json"), "{}").unwrap();
    let settings = workspace.join("pnpm-workspace.yaml");
    let original = "permissions:\n  example:\n    build: false\n";
    for (keys, grants_skills) in [("q", false), ("\r", false), ("j \r", true)] {
        fs::write(&settings, original).unwrap();
        pacquet_in_terminal(&workspace)
            .with_env("PNPM_TEST_PROMPT_TEXT", "Provide agent skills")
            .with_env("PNPM_TEST_PROMPT_ANSWER", keys)
            .with_args(["approve", "example", "example"])
            .assert()
            .success();
        let policy = fs::read_to_string(&settings).unwrap();
        if grants_skills {
            assert!(policy.contains("skills: true"), "{policy}");
            assert!(policy.contains("build: false"), "{policy}");
        } else {
            assert_eq!(policy, original);
        }
    }
    drop(root);
}
