use super::{describe_pin, record_package_manager_pin};
use crate::engine_pm::channel::PackageManager;
use serde_json::json;

#[test]
fn a_pin_is_recorded_under_dev_engines() {
    let mut manifest = json!({ "name": "project", "version": "1.0.0" });
    record_package_manager_pin(&mut manifest, PackageManager::Yarn, Some("4"));
    assert_eq!(
        manifest,
        json!({
            "name": "project",
            "version": "1.0.0",
            "devEngines": { "packageManager": { "name": "yarn", "version": "4" } },
        }),
    );
}

/// The field holds a range, so a request that named no version records
/// the package manager without inventing one.
#[test]
fn a_request_without_a_version_records_only_the_name() {
    let mut manifest = json!({ "name": "project" });
    record_package_manager_pin(&mut manifest, PackageManager::Bun, None);
    assert_eq!(manifest["devEngines"]["packageManager"], json!({ "name": "bun" }));
    assert_eq!(describe_pin(PackageManager::Bun, None), "bun");
    assert_eq!(describe_pin(PackageManager::Yarn, Some("4")), "yarn@4");
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
    record_package_manager_pin(&mut manifest, PackageManager::Yarn, Some("4.9.2"));
    assert_eq!(
        manifest["devEngines"],
        json!({
            "runtime": { "name": "node", "version": "22" },
            "packageManager": { "name": "yarn", "version": "4.9.2" },
        }),
    );
}
