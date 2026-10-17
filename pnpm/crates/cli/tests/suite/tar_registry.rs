use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::fs;

const PARENT: &str = "@pnpm.e2e/pkg-with-1-dep";
const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";

/// A project that resolves every package from the mock registry's tar
/// registry, `/~zstd/`.
fn tar_registry_project() -> (CommandTempCwd<AddMockedRegistry>, String) {
    let env = CommandTempCwd::init().add_mocked_registry();
    let tar_registry =
        format!("{}~{}/", env.npmrc_info.mock_instance.url(), pnpr_fixtures::TAR_REGISTRY);
    let workspace_yaml = env.workspace.join("pnpm-workspace.yaml");
    let settings = format!(
        "{}registries:\n  {tar_registry}:\n    scopes: ['@']\n    serverType: pnpr\n",
        fs::read_to_string(&workspace_yaml).unwrap(),
    );
    fs::write(&workspace_yaml, settings).unwrap();
    fs::write(
        env.workspace.join("package.json"),
        serde_json::json!({ "dependencies": { PARENT: "100.0.0" } }).to_string(),
    )
    .unwrap();
    (env, tar_registry)
}

#[test]
fn installs_from_a_tar_registry_and_pins_the_tar_hashes() {
    let (env, tar_registry) = tar_registry_project();
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = env;
    pacquet
        .with_arg("install")
        .assert()
        .success();
    assert!(
        workspace
            .join("node_modules")
            .join(PARENT)
            .join("package.json")
            .is_file(),
    );

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap();
    eprintln!("{lockfile}");
    assert!(!lockfile.contains("tarball:"), "the pnpr layout lets the lockfile omit tarball URLs");
    let lockfile: serde_json::Value = serde_saphyr::from_str(&lockfile).unwrap();
    let packages = lockfile["packages"].as_object().unwrap();
    assert!(
        packages
            .keys()
            .any(|key| key.starts_with(DEP)),
        "{packages:?}",
    );
    for (key, package) in packages {
        let integrity = package["resolution"]["integrity"]
            .as_str()
            .unwrap_or_else(|| panic!("{key} has an integrity"));
        let tar = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { reqwest::get(format!("{tar_registry}{key}")).await?.bytes().await })
            .unwrap();
        let mut expected = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
        expected.input(&tar);
        assert_eq!(integrity, expected.result().to_string(), "{key} pins the hash of its tar");
    }

    // A frozen install into an empty store verifies every tar against the pins.
    fs::remove_dir_all(workspace.join("node_modules")).unwrap();
    fs::remove_dir_all(&npmrc_info.store_dir).unwrap();
    crate::_utils::pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert!(
        workspace
            .join("node_modules")
            .join(PARENT)
            .join("package.json")
            .is_file(),
    );
    drop((root, npmrc_info));
}

#[test]
fn a_lockfile_only_resolution_records_the_hash_a_tar_registry_omits() {
    let (env, _) = tar_registry_project();
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = env;
    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    let lockfile: serde_json::Value =
        serde_saphyr::from_str(&fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap())
            .unwrap();
    for (key, package) in lockfile["packages"].as_object().unwrap() {
        assert!(package["resolution"]["integrity"].is_string(), "{key} has an integrity");
    }
    drop((root, npmrc_info));
}
