use pacquet_package_manifest::DependencyGroup;

use super::{RuntimeArgs, RuntimeError};

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
fn global_is_rejected_before_state_initialization() {
    let err = RuntimeArgs { global: true, ..args(&["set", "node", "22"]) }
        .reject_unsupported_global()
        .unwrap_err();
    assert_eq!(err, RuntimeError::GlobalUnsupported);
}
