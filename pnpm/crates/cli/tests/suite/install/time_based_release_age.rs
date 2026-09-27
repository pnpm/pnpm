//! `resolutionMode: time-based` combined with `minimumReleaseAge`: only the
//! `minimumReleaseAge` cutoff decides what counts as a release-age violation.

use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, append_workspace_yaml_key,
    bravo_mature_bravo_dep_1_1_0_immature_minimum_release_age, fs, set_minimum_release_age,
};
use assert_cmd::assert::OutputAssertExt;
use pnpm_testing_utils::diagnostics::assert_diagnostic_contains;

/// Install `@pnpm.e2e/bravo@1.0.0` under `time-based` with its dependency
/// `@pnpm.e2e/bravo-dep` overridden to 1.1.0. Bravo is published in 2022-04,
/// so `time-based` resolves its dependencies against a cutoff one hour later,
/// and bravo-dep@1.1.0, published in 2022-05, is newer than that cutoff.
fn time_based_install_with_newer_subdep(
    minimum_release_age: u64,
    strict: bool,
) -> CommandTempCwd<AddMockedRegistry> {
    let setup = CommandTempCwd::init().add_mocked_registry();
    let workspace = &setup.workspace;
    append_workspace_yaml_key(workspace, "resolutionMode", "time-based");
    set_minimum_release_age(workspace, minimum_release_age);
    append_workspace_yaml_key(workspace, "minimumReleaseAgeStrict", strict);
    append_workspace_yaml_key(workspace, "overrides", "\n  \"@pnpm.e2e/bravo-dep\": 1.1.0");
    let package_json_content = serde_json::json!({
        "dependencies": { "@pnpm.e2e/bravo": "1.0.0" },
    });
    fs::write(workspace.join("package.json"), package_json_content.to_string())
        .expect("write to package.json");
    setup
}

#[test]
fn strict_install_accepts_a_subdep_newer_than_only_the_time_based_cutoff() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = time_based_install_with_newer_subdep(1, true);

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let bravo_dep = workspace.join("node_modules/.pnpm/@pnpm.e2e+bravo-dep@1.1.0");
    assert!(bravo_dep.exists());

    drop((root, npmrc_info));
}

#[test]
fn loose_install_does_not_exclude_a_subdep_newer_than_only_the_time_based_cutoff() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = time_based_install_with_newer_subdep(1, false);

    pacquet
        .with_arg("install")
        .assert()
        .success();

    let yaml = fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
        .expect("read pnpm-workspace.yaml");
    eprintln!("pnpm-workspace.yaml:\n{yaml}");
    assert!(!yaml.contains("minimumReleaseAgeExclude"));

    drop((root, npmrc_info));
}

#[test]
fn strict_install_reports_a_time_based_subdep_against_the_minimum_release_age_cutoff() {
    let CommandTempCwd { pacquet, root, npmrc_info, .. } = time_based_install_with_newer_subdep(
        bravo_mature_bravo_dep_1_1_0_immature_minimum_release_age(),
        true,
    );

    let output = pacquet
        .with_arg("install")
        .output()
        .expect("run pnpm install");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("{stderr}");
    assert!(!output.status.success());
    assert_diagnostic_contains(&stderr, "ERR_PNPM_NO_MATURE_MATCHING_VERSION");
    assert_diagnostic_contains(&stderr, "@pnpm.e2e/bravo-dep@1.1.0");
    assert_diagnostic_contains(&stderr, "(2022-04-15T");

    drop((root, npmrc_info));
}
