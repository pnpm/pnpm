use super::{package_manager_to_sync, version_satisfies, wanted_package_manager};
use pnpm_config::PNPM_VERSION;
use std::path::Path;

fn dev_engines_manifest(version: &str) -> serde_json::Value {
    serde_json::json!({
        "devEngines": {
            "packageManager": { "name": "pnpm", "version": version, "onFail": "download" },
        },
    })
}

/// Corepack writes the artifact it downloaded as a build on the version it
/// pins. The resolved version never carries it, so a pin that kept it would
/// record a version no later run recognizes: every install would rewrite the
/// same entry, and every `--frozen-lockfile` run would reject it.
#[test]
fn a_corepack_build_is_not_part_of_the_pinned_version() {
    let manifest = dev_engines_manifest(&format!("{PNPM_VERSION}+sha512.0123456789abcdef"));

    let pin = format!("{PNPM_VERSION}+sha512.0123456789abcdef");

    let sync = package_manager_to_sync(&manifest, Path::new("."), None).expect("an entry to sync");

    assert_eq!(sync.version, PNPM_VERSION);
    // The specifier records the pin as the manifest writes it, the way the
    // TypeScript CLI does — both stacks write the same lockfile.
    assert_eq!(sync.specifier, pin);
}

/// A range is not a version, and its `+` (if any) is not corepack's.
#[test]
fn a_range_pin_is_left_as_written() {
    let manifest = dev_engines_manifest(">=9.1.0 <9.1.2");

    let pm = wanted_package_manager(&manifest).expect("a pinned package manager");

    assert_eq!(pm.version.as_deref(), Some(">=9.1.0 <9.1.2"));
    assert!(package_manager_to_sync(&manifest, Path::new("."), None).is_none());
}

/// Every expectation is npm's own answer, read off
/// `semver.satisfies(version, range, { includePrerelease: true })` — the
/// call the TypeScript CLI checks the same pins with.
#[test]
fn a_version_is_matched_against_a_pin_the_way_npm_matches_it() {
    let cases = [
        ("12.0.0-rc.7", "^12.0.0-rc.3", true),
        ("12.0.0-rc.7", "12.0.0-rc.7", true),
        ("12.0.0-rc.7", "^12", true),
        ("12.0.0-rc.7", ">=12", true),
        ("12.0.0-rc.7", "^11 || ^12", true),
        // A prerelease sorts below its own release, so a pin naming a later
        // one — or the release itself — leaves it behind.
        ("12.0.0-rc.7", ">=12.0.0-rc.9", false),
        ("12.0.0-rc.7", "^12.0.0-rc.11", false),
        ("12.0.0-rc.7", "12.0.0", false),
        ("12.0.0-rc.7", "^12.0.0", false),
        ("12.0.0-rc.7", ">=12.0.0", false),
        ("12.0.0", "^12.0.0", true),
        ("22.5.0", "<=22", true),
        ("not a version", "*", false),
    ];

    for (version, pin, satisfies) in cases {
        assert_eq!(version_satisfies(version, pin), satisfies, "{version} against {pin}");
    }
}
