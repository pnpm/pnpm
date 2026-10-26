//! A plain lockfile key names no registry. When its scope is routed to
//! another registry, a resolving install resolves the package and its
//! locked children again from the new registry, so a version the new
//! registry does not serve is not pinned from the old one.

use crate::_utils::{append_workspace_yaml_key, pacquet_in, read_lockfile};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_lockfile::Lockfile;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    registry::TestRegistry,
};
use pretty_assertions::assert_eq;
use std::{fs, path::Path};
use tempfile::TempDir;

/// `@pnpm.e2e/foobar@100.0.0` declares `@pnpm.e2e/foo: ^100.0.0`, a plain
/// range the `@pnpm.e2e` scope routes.
const PARENT: &str = "@pnpm.e2e/foobar";
const CHILD: &str = "@pnpm.e2e/foo";

fn setup() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    (root, workspace, npmrc_info)
}

/// Route the `@pnpm.e2e` scope to `registry`, replacing an earlier route.
fn route_scope(workspace: &Path, registry: &str) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    let kept: Vec<&str> = yaml
        .lines()
        .filter(|line| !line.starts_with("registries:"))
        .collect();
    fs::write(&yaml_path, format!("{}\n", kept.join("\n"))).expect("write pnpm-workspace.yaml");
    append_workspace_yaml_key(workspace, "registries", format!("{{ '@pnpm.e2e': '{registry}' }}"));
}

/// The sorted `packages:` keys recorded for `name`.
fn package_keys(lockfile: &Lockfile, name: &str) -> Vec<String> {
    let mut keys: Vec<String> = lockfile.packages
        .iter()
        .flatten()
        .filter(|(key, _)| key.name.to_string() == name)
        .map(|(key, _)| key.to_string())
        .collect();
    keys.sort();
    keys
}

fn recorded_registry_routes(
    workspace: &Path,
) -> Option<std::collections::BTreeMap<String, String>> {
    pnpm_workspace_state::load_workspace_state(workspace)
        .expect("read the workspace state")
        .expect("a workspace state")
        .settings
        .registry_routes
}

/// The child's locked 100.1.0 is not served by the registry the scope moves
/// to, so the resolving install picks the version it serves, from the
/// parent's range.
#[test]
fn moving_a_scope_re_resolves_its_plain_keyed_packages_from_the_new_registry() {
    let (root, workspace, anchor) = setup();
    let old = anchor.mock_instance.url().to_string();
    let new = TestRegistry::start_with_own_storage(root.path());
    new.remove_version(CHILD, "100.1.0");
    route_scope(&workspace, &old);
    fs::write(
        workspace.join("package.json"),
        format!(r#"{{ "name": "test-registry-routes", "version": "1.0.0", "dependencies": {{ "{PARENT}": "100.0.0" }} }}"#),
    )
    .expect("write package.json");
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let lockfile = read_lockfile(&workspace.join(Lockfile::FILE_NAME));
    assert_eq!(package_keys(&lockfile, CHILD), ["@pnpm.e2e/foo@100.1.0"]);
    assert_eq!(
        recorded_registry_routes(&workspace)
            .expect("the install records the routes")
            .get("@pnpm.e2e"),
        Some(&old),
    );

    route_scope(&workspace, new.url());
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let lockfile = read_lockfile(&workspace.join(Lockfile::FILE_NAME));
    assert_eq!(package_keys(&lockfile, PARENT), ["@pnpm.e2e/foobar@100.0.0"]);
    assert_eq!(package_keys(&lockfile, CHILD), ["@pnpm.e2e/foo@100.0.0"]);
    assert_eq!(
        recorded_registry_routes(&workspace)
            .expect("the install records the routes")
            .get("@pnpm.e2e"),
        Some(&new.url().to_string()),
    );

    drop((root, anchor, new));
}

/// A frozen install installs the lockfile as recorded, and leaves the record
/// of where its packages came from for the resolving install that follows.
#[test]
fn a_frozen_install_keeps_the_moved_packages_and_the_recorded_routes() {
    let (root, workspace, anchor) = setup();
    let old = anchor.mock_instance.url().to_string();
    let new = TestRegistry::start_with_own_storage(root.path());
    route_scope(&workspace, &old);
    fs::write(
        workspace.join("package.json"),
        format!(r#"{{ "name": "test-registry-routes", "version": "1.0.0", "dependencies": {{ "{PARENT}": "100.0.0" }} }}"#),
    )
    .expect("write package.json");
    pacquet_in(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let written = fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read lockfile");

    route_scope(&workspace, new.url());
    pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read lockfile"),
        written,
    );
    assert_eq!(
        recorded_registry_routes(&workspace)
            .expect("the install records the routes")
            .get("@pnpm.e2e"),
        Some(&old),
        "the plain keys still come from the old registry",
    );

    drop((root, anchor, new));
}
