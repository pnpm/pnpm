use pacquet_config::Config;
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::SilentReporter;
use tempfile::tempdir;

use super::{RuntimeArgs, RuntimeError};
use crate::State;

fn args(params: &[&str]) -> RuntimeArgs {
    RuntimeArgs {
        global: false,
        save_dev: false,
        save_prod: false,
        params: params.iter().map(|param| (*param).to_string()).collect(),
    }
}

#[test]
fn set_request_defaults_to_dev_engines_runtime() {
    let request = args(&["set", "node", "22"]).set_request().unwrap();
    assert_eq!(request.package_name, "node@runtime:22");
    assert_eq!(request.dependency_group, DependencyGroup::Dev);
}

#[test]
fn set_request_saves_prod_when_save_prod_is_set() {
    let request =
        RuntimeArgs { save_prod: true, ..args(&["set", "node", "22"]) }.set_request().unwrap();
    assert_eq!(request.package_name, "node@runtime:22");
    assert_eq!(request.dependency_group, DependencyGroup::Prod);
}

#[test]
fn set_request_prefers_save_dev_over_save_prod() {
    let request = RuntimeArgs { save_dev: true, save_prod: true, ..args(&["set", "node", "22"]) }
        .set_request()
        .unwrap();
    assert_eq!(request.package_name, "node@runtime:22");
    assert_eq!(request.dependency_group, DependencyGroup::Dev);
}

#[test]
fn set_request_allows_missing_version_spec() {
    let request = args(&["set", "node"]).set_request().unwrap();
    assert_eq!(request.package_name, "node@runtime:");
    assert_eq!(request.dependency_group, DependencyGroup::Dev);
}

#[test]
fn set_request_works_with_deno() {
    let request = args(&["set", "deno", "2"]).set_request().unwrap();
    assert_eq!(request.package_name, "deno@runtime:2");
    assert_eq!(request.dependency_group, DependencyGroup::Dev);
}

#[test]
fn set_request_fails_without_subcommand() {
    let err = args(&[]).set_request().unwrap_err();
    assert_eq!(err, RuntimeError::NoSubcommand);
}

#[test]
fn set_request_fails_for_unknown_subcommand() {
    let err = args(&["foo"]).set_request().unwrap_err();
    assert_eq!(err, RuntimeError::UnknownSubcommand { subcommand: "foo".to_string() });
}

#[test]
fn set_request_fails_without_runtime_name() {
    let err = args(&["set"]).set_request().unwrap_err();
    assert_eq!(err, RuntimeError::MissingRuntimeName);
}

#[test]
fn set_request_rejects_unsupported_runtime_names() {
    // An unknown runtime, plus the comma-list and local-path forms the
    // global-add pipeline would otherwise misread as extra install targets.
    for name in ["python", "node,is-positive", "./evil", "file:./evil"] {
        let err = args(&["set", name, "22"]).set_request().unwrap_err();
        assert_eq!(err, RuntimeError::InvalidRuntimeName { name: name.to_string() });
    }
}

#[test]
fn set_request_rejects_a_comma_in_the_version() {
    // The version is interpolated into the comma-splittable selector, so a
    // comma could smuggle in a second global install target.
    let err = args(&["set", "node", "22,is-positive"]).set_request().unwrap_err();
    assert_eq!(err, RuntimeError::InvalidRuntimeVersion { version: "22,is-positive".to_string() });
}

#[test]
fn set_request_accepts_every_supported_runtime() {
    for name in ["node", "deno", "bun"] {
        let request = args(&["set", name, "22"]).set_request().unwrap();
        assert_eq!(request.package_name, format!("{name}@runtime:22"));
    }
}

/// `run` (the local, non-`-g` path) must actually reach the shared
/// `add_package` pipeline with the request `set_request` built, not just
/// build the request and stop. `offline` makes the node resolver fail
/// fast (no network) as soon as it tries to resolve `node@runtime:22`,
/// which only happens once `run` has handed the request off.
#[tokio::test]
async fn run_hands_the_set_request_off_to_add_package() {
    let dir = tempdir().expect("temp dir");
    let mut config = Config::new();
    config.store_dir = dir.path().join("pacquet-store").into();
    config.modules_dir = dir.path().join("node_modules");
    config.virtual_store_dir = config.modules_dir.join(".pacquet");
    config.offline = true;
    let config = config.leak();
    let state = State::init(dir.path().join("package.json"), config, false).expect("init state");

    let err = args(&["set", "node", "22"])
        .run::<SilentReporter>(state)
        .await
        .expect_err("offline resolution must fail, not silently succeed");
    let err = format!("{err:?}");
    assert!(err.contains("Offline"), "expected the offline Node.js resolver error, got: {err}");
}

#[test]
fn global_install_builds_the_same_runtime_selector() {
    // `--global` routes through `run_global`, which reuses `set_request`
    // to build the `<name>@runtime:<version>` selector. The
    // `--save-dev` / `--save-prod` group is irrelevant globally (the
    // global group always saves to `dependencies`), so only the selector
    // is asserted here.
    let request =
        RuntimeArgs { global: true, ..args(&["set", "node", "22"]) }.set_request().unwrap();
    assert_eq!(request.package_name, "node@runtime:22");
}
