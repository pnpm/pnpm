use super::{assert_success, pacquet_at, run, write_manifest};
use command_extra::CommandExtra;
use pnpm_config::PNPM_VERSION;
use pnpm_lockfile::EnvLockfile;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::fs;

#[test]
fn a_legacy_switch_records_the_pin_for_a_subsequent_frozen_install() {
    for manifest in [
        json!({ "packageManager": format!("pnpm@{PNPM_VERSION}") }),
        json!({ "devEngines": { "packageManager": {
            "name": "pnpm", "version": PNPM_VERSION, "onFail": "download",
        } } }),
    ] {
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry_with_pnpm_version(PNPM_VERSION);
        write_manifest(&workspace, &manifest);
        let command = || {
            pacquet_at(&workspace).with_env("PNPM_CONFIG_REGISTRY", npmrc_info.mock_instance.url())
        };

        // pnpm 10 sets both variables when it delegates to the project's pnpm.
        let output = run(
            command()
                .with_env("npm_config_manage_package_manager_versions", "false")
                .with_env("pnpm_config_pm_on_fail", "ignore"),
            root.path(),
            &["install", "--lockfile-only"],
        );
        assert_success(&output);

        let env = EnvLockfile::read(&workspace).unwrap().expect("env lockfile");
        let recorded = env.importers[EnvLockfile::ROOT_IMPORTER_KEY]
            .package_manager_dependencies
            .as_ref()
            .expect("packageManagerDependencies should be recorded");
        assert_eq!(recorded["pnpm"].specifier, PNPM_VERSION);
        assert_eq!(recorded["pnpm"].version, PNPM_VERSION);
        let lockfile = fs::read(workspace.join("pnpm-lock.yaml")).unwrap();

        let output = run(command(), root.path(), &["install", "--frozen-lockfile"]);
        assert_success(&output);
        assert_eq!(fs::read(workspace.join("pnpm-lock.yaml")).unwrap(), lockfile);
        drop((root, npmrc_info));
    }
}

#[test]
fn ignored_pins_outside_a_compatible_legacy_switch_remain_unrecorded() {
    for (manifest, version_management) in [
        (json!({ "packageManager": format!("pnpm@{PNPM_VERSION}") }), "true"),
        (json!({ "packageManager": "pnpm@99.0.0" }), "false"),
        (json!({ "packageManager": "yarn@4.0.0" }), "false"),
        (
            json!({ "devEngines": { "packageManager": {
                "name": "pnpm", "version": PNPM_VERSION, "onFail": "ignore",
            } } }),
            "false",
        ),
    ] {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        write_manifest(&workspace, &manifest);
        let output = run(
            pacquet
                .with_env("npm_config_manage_package_manager_versions", version_management)
                .with_env("pnpm_config_pm_on_fail", "ignore"),
            root.path(),
            &["install", "--lockfile-only"],
        );
        assert_success(&output);
        let env = EnvLockfile::read(&workspace).unwrap();
        dbg!(&manifest, &env);
        assert!(env.is_none_or(|env| {
            env.importers
                .get(EnvLockfile::ROOT_IMPORTER_KEY)
                .is_none_or(|importer| importer.package_manager_dependencies.is_none())
        }));
    }
}

#[test]
fn a_with_handoff_keeps_ignoring_a_compatible_pin() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, &json!({ "packageManager": format!("pnpm@{PNPM_VERSION}") }));
    let output = run(
        pacquet
            .with_env("npm_config_manage_package_manager_versions", "false")
            .with_env("pnpm_config_pm_on_fail", "ignore")
            .with_env("COREPACK_ROOT", "pnpm-with"),
        root.path(),
        &["install", "--lockfile-only"],
    );
    assert_success(&output);
    let env = EnvLockfile::read(&workspace).unwrap();
    dbg!(&env);
    assert!(env.is_none());
}
