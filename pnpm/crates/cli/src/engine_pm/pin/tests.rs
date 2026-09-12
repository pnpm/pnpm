use super::{declared_package_manager, describe_pin, record_package_manager_pin};
use crate::engine_pm::channel::PackageManager;
use serde_json::json;

/// Yarn is started from `packageManager`, in corepack's `<name>@<version>`
/// spelling.
#[test]
fn a_yarn_pin_is_recorded_in_the_package_manager_field() {
    let mut manifest = json!({ "name": "project", "version": "1.0.0" });
    record_package_manager_pin(
        manifest.as_object_mut().unwrap(),
        PackageManager::Yarn,
        Some("4.9.2"),
    );
    assert_eq!(
        manifest,
        json!({ "name": "project", "version": "1.0.0", "packageManager": "yarn@4.9.2" }),
    );
}

#[test]
fn another_package_manager_is_recorded_under_dev_engines() {
    let mut manifest = json!({ "name": "project", "version": "1.0.0" });
    record_package_manager_pin(manifest.as_object_mut().unwrap(), PackageManager::Npm, Some("11"));
    assert_eq!(
        manifest,
        json!({
            "name": "project",
            "version": "1.0.0",
            "devEngines": { "packageManager": { "name": "npm", "version": "11" } },
        }),
    );
}

/// The field holds a range, so a request that named no version records
/// the package manager without inventing one.
#[test]
fn a_request_without_a_version_records_only_the_name() {
    let mut manifest = json!({ "name": "project" });
    record_package_manager_pin(manifest.as_object_mut().unwrap(), PackageManager::Bun, None);
    assert_eq!(manifest["devEngines"]["packageManager"], json!({ "name": "bun" }));
    assert_eq!(describe_pin(PackageManager::Bun, None), "bun");
    assert_eq!(describe_pin(PackageManager::Yarn, Some("4.9.2")), "yarn@4.9.2");
}

/// Other `devEngines` entries are the project's own and stay put; the
/// package manager is the only one this replaces.
#[test]
fn recording_a_pin_keeps_the_rest_of_dev_engines() {
    let mut manifest = json!({
        "devEngines": {
            "runtime": { "name": "node", "version": "22" },
            "packageManager": { "name": "npm", "version": "10" },
        },
    });
    record_package_manager_pin(
        manifest.as_object_mut().unwrap(),
        PackageManager::Bun,
        Some("1.3.0"),
    );
    assert_eq!(
        manifest["devEngines"],
        json!({
            "runtime": { "name": "node", "version": "22" },
            "packageManager": { "name": "bun", "version": "1.3.0" },
        }),
    );
}

/// The two fields declare the same thing, and corepack refuses to run a
/// project whose declarations disagree — so recording one clears the
/// other, in both directions.
#[test]
fn recording_a_pin_clears_the_declaration_it_replaces() {
    let mut manifest = json!({
        "packageManager": "pnpm@12.0.0",
        "devEngines": { "packageManager": { "name": "npm", "version": "10" } },
    });
    record_package_manager_pin(
        manifest.as_object_mut().unwrap(),
        PackageManager::Yarn,
        Some("4.9.2"),
    );
    assert_eq!(manifest, json!({ "packageManager": "yarn@4.9.2" }));

    record_package_manager_pin(manifest.as_object_mut().unwrap(), PackageManager::Npm, Some("11"));
    assert_eq!(
        manifest,
        json!({ "devEngines": { "packageManager": { "name": "npm", "version": "11" } } }),
    );
}

/// A `devEngines` that still declares something else survives the Yarn
/// pin that empties its package manager.
#[test]
fn clearing_the_package_manager_keeps_a_dev_engines_runtime() {
    let mut manifest = json!({
        "devEngines": {
            "runtime": { "name": "node", "version": "22" },
            "packageManager": { "name": "npm", "version": "10" },
        },
    });
    record_package_manager_pin(
        manifest.as_object_mut().unwrap(),
        PackageManager::Yarn,
        Some("4.9.2"),
    );
    assert_eq!(
        manifest,
        json!({
            "devEngines": { "runtime": { "name": "node", "version": "22" } },
            "packageManager": "yarn@4.9.2",
        }),
    );
}

/// A specifier that locates a package to install under the package
/// manager's name is an ordinary dependency, not a declaration of which
/// package manager the project uses.
#[test]
fn a_located_package_is_not_a_package_manager_declaration() {
    for request in [
        "yarn@npm:@yarnpkg/cli-dist@4.9.2",
        "yarn@yarnpkg/berry",
        "yarn@yarnpkg/berry#main",
        "npm@github:npm/cli",
        "bun@https://example.test/bun.tgz",
        "yarn@file:../yarn",
    ] {
        assert_eq!(declared_package_manager(request), None, "{request}");
    }
}

/// A version, a range and a dist-tag all ask for a released version of
/// the package manager itself.
#[test]
fn a_version_request_declares_the_package_manager() {
    let declared = declared_package_manager;
    assert_eq!(declared("yarn"), Some((PackageManager::Yarn, None)));
    assert_eq!(declared("yarn@4"), Some((PackageManager::Yarn, Some("4".to_string()))));
    assert_eq!(declared("npm@^11.1.0"), Some((PackageManager::Npm, Some("^11.1.0".to_string()))));
    assert_eq!(declared("bun@latest"), Some((PackageManager::Bun, Some("latest".to_string()))));
    // pnpm's own pin is `pnpm self-update`'s to change.
    assert_eq!(declared("pnpm@12"), None);
    assert_eq!(declared("typescript@5"), None);
}
