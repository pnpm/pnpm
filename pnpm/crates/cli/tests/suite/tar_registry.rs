use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

const PARENT: &str = "@pnpm.e2e/pkg-with-1-dep";
const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";

/// A project depending on [`PARENT`], served by a mock registry of its own so
/// the tar hashes it derives start empty on every run.
fn project() -> (CommandTempCwd<AddMockedRegistry>, String) {
    let env = CommandTempCwd::init().add_mocked_registry_with_own_storage();
    let tar_registry =
        format!("{}~{}/", env.npmrc_info.mock_instance.url(), pnpr_fixtures::TAR_REGISTRY);
    fs::write(
        env.workspace.join("package.json"),
        serde_json::json!({ "dependencies": { PARENT: "100.0.0" } }).to_string(),
    )
    .unwrap();
    (env, tar_registry)
}

/// Route every package to the mock registry's tar registry, `/~zstd/`.
fn use_tar_registry(workspace: &Path, tar_registry: &str, server_type: Option<&str>) {
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let server_type =
        server_type.map_or_else(String::new, |value| format!("    serverType: {value}\n"));
    let settings = format!(
        "{}registries:\n  {tar_registry}:\n    scopes: ['@']\n{server_type}",
        fs::read_to_string(&workspace_yaml).unwrap(),
    );
    fs::write(&workspace_yaml, settings).unwrap();
}

fn get(url: &str) -> Vec<u8> {
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { reqwest::get(url).await?.error_for_status()?.bytes().await })
        .unwrap()
        .to_vec()
}

fn read_lockfile(workspace: &Path) -> serde_json::Value {
    serde_saphyr::from_str(&fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap()).unwrap()
}

fn sha512(bytes: &[u8]) -> String {
    let mut digest = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    digest.input(bytes);
    digest.result().to_string()
}

/// Every lockfile entry pins the hash of the tar the registry serves for it.
fn assert_pins_tar_hashes(workspace: &Path, tar_registry: &str) {
    let lockfile = read_lockfile(workspace);
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
        let tar = get(&format!("{tar_registry}{key}"));
        assert_eq!(integrity, sha512(&tar), "{key} pins the hash of its tar");
    }
}

#[test]
fn installs_from_a_tar_registry_and_pins_the_tar_hashes() {
    let (env, tar_registry) = project();
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = env;
    use_tar_registry(&workspace, &tar_registry, Some("pnpr"));
    let packument: serde_json::Value =
        serde_json::from_slice(&get(&format!("{tar_registry}@pnpm.e2e%2Fpkg-with-1-dep"))).unwrap();
    assert!(
        packument["versions"]["100.0.0"]["dist"].get("integrity").is_none(),
        "the registry has not derived the tar hash yet, so the install has to: {packument}",
    );

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
    assert!(!lockfile.contains("tarball:"), "the pnpr layout lets the lockfile omit tarball URLs");
    assert_pins_tar_hashes(&workspace, &tar_registry);

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

/// An entry routed only through the bare `@` scope is the default registry:
/// the install resolves through it with the entry's `serverType`, so it must
/// not be reported as an entry that matches no registry.
/// <https://github.com/pnpm/tasks/issues/184>
#[test]
fn an_entry_routed_through_the_default_scope_is_not_reported_as_unmatched() {
    let (env, tar_registry) = project();
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = env;
    use_tar_registry(&workspace, &tar_registry, Some("pnpr"));
    let output = pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();
    println!("{stderr}");
    assert!(!stderr.contains("do not match any configured registry"), "{stderr}");
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap();
    assert!(!lockfile.contains("tarball:"), "the entry's serverType applied: {lockfile}");
    assert_pins_tar_hashes(&workspace, &tar_registry);
    drop((root, npmrc_info));
}

#[test]
fn a_lockfile_only_resolution_records_the_hash_a_tar_registry_omits() {
    let (env, tar_registry) = project();
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = env;
    use_tar_registry(&workspace, &tar_registry, Some("pnpr"));
    pacquet
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    assert_pins_tar_hashes(&workspace, &tar_registry);
    drop((root, npmrc_info));
}

/// Moving a project from a `.tgz` registry to a tar registry changes what the
/// registry serves, so the recorded hashes fail until they are refreshed.
#[test]
fn switching_to_a_tar_registry_needs_update_checksums() {
    let (env, tar_registry) = project();
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

    use_tar_registry(&workspace, &tar_registry, Some("pnpr"));
    fs::remove_dir_all(workspace.join("node_modules")).unwrap();
    fs::remove_dir_all(&npmrc_info.store_dir).unwrap();
    let frozen = crate::_utils::pacquet_in(&workspace)
        .with_env("PNPM_CONFIG_FETCH_RETRIES", "0")
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&frozen.get_output().stderr).into_owned();
    assert!(stderr.contains("ERR_PNPM_TARBALL_INTEGRITY"), "{stderr}");

    crate::_utils::pacquet_in(&workspace)
        .with_args(["install", "--update-checksums"])
        .assert()
        .success();
    assert_pins_tar_hashes(&workspace, &tar_registry);
    drop((root, npmrc_info));
}

#[test]
fn a_registry_that_omits_tarball_urls_needs_a_declared_layout() {
    let (env, tar_registry) = project();
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = env;
    use_tar_registry(&workspace, &tar_registry, None);
    let output = pacquet
        .with_arg("install")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();
    assert!(stderr.contains("names no tarball URL"), "{stderr}");
    drop((root, npmrc_info));
}
