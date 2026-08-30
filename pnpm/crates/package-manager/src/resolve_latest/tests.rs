use pnpm_registry::PackageVersion;
use serde_json::json;

use super::{exact_pins, pin_is_exempt};

/// Built by deserialization, the way a packument entry reaches the picker,
/// so the fixture does not have to name every field of `PackageVersion`.
fn candidate(
    dependencies: &serde_json::Value,
    optional_dependencies: &serde_json::Value,
) -> PackageVersion {
    serde_json::from_value(json!({
        "name": "parent",
        "version": "1.0.0",
        "dist": { "tarball": "https://registry.npmjs.org/parent/-/parent-1.0.0.tgz", "shasum": "" },
        "dependencies": dependencies,
        "optionalDependencies": optional_dependencies,
    }))
    .expect("valid package version")
}

fn pins(candidate: &PackageVersion) -> Vec<(String, String)> {
    let mut pins: Vec<(String, String)> = exact_pins(candidate)
        .map(|(name, version)| (name.to_string(), version.to_string()))
        .collect();
    pins.sort();
    pins
}

#[test]
fn reports_exact_pins_from_both_dependency_groups() {
    // Optional dependencies count: the lockfile records every platform's
    // binary, so one too young blocks the install on every platform.
    let candidate = candidate(&json!({ "oxc": "0.146.0" }), &json!({ "binding-darwin": "1.2.5" }));

    assert_eq!(
        pins(&candidate),
        vec![
            ("binding-darwin".to_string(), "1.2.5".to_string()),
            ("oxc".to_string(), "0.146.0".to_string()),
        ],
    );
}

#[test]
fn passes_over_dependencies_a_range_can_satisfy() {
    // A range has other versions to fall back on, and choosing among them is
    // the install's resolution, not this pre-check's.
    let candidate = candidate(
        &json!({ "caret": "^1.0.0", "tilde": "~2.3.4", "any": "*", "exact": "3.0.0" }),
        &json!({}),
    );

    assert_eq!(pins(&candidate), vec![("exact".to_string(), "3.0.0".to_string())]);
}

#[test]
fn passes_over_specifiers_that_name_no_registry_version() {
    // `=1.0.0` is a range that happens to admit one version, and the rest
    // resolve through other protocols entirely. None can be looked up by
    // version in a packument, so none is judged here.
    let candidate = candidate(
        &json!({
            "equals": "=1.0.0",
            "tag": "latest",
            "workspace": "workspace:*",
            "git": "github:owner/repo",
        }),
        &json!({}),
    );

    assert_eq!(pins(&candidate), Vec::<(String, String)>::new());
}

#[test]
fn reports_nothing_for_a_candidate_that_declares_no_dependencies() {
    assert_eq!(pins(&candidate(&json!({}), &json!({}))), Vec::<(String, String)>::new());
}

#[test]
fn an_optional_declaration_overrides_a_same_name_dependency() {
    // npm resolves a name declared in both groups to its optional entry, so
    // the `dependencies` specifier never reaches the install and must not
    // decide whether the candidate is passed over.
    let candidate = candidate(
        &json!({ "shared": "1.0.0" }),
        &json!({ "shared": "^2.0.0", "binding": "1.2.5" }),
    );

    assert_eq!(pins(&candidate), vec![("binding".to_string(), "1.2.5".to_string())]);
}

#[test]
fn an_optional_override_that_is_itself_a_pin_is_the_one_judged() {
    let candidate = candidate(&json!({ "shared": "1.0.0" }), &json!({ "shared": "2.0.0" }));

    assert_eq!(pins(&candidate), vec![("shared".to_string(), "2.0.0".to_string())]);
}

fn exclude(patterns: &[&str]) -> pnpm_config::version_policy::PackageVersionPolicy {
    pnpm_config::version_policy::create_package_version_policy(patterns)
        .expect("valid exclude patterns")
}

#[test]
fn a_name_only_exclusion_exempts_every_version_of_that_package() {
    let policy = exclude(&["child"]);

    assert!(pin_is_exempt(Some(&policy), "child", "1.0.0"));
    assert!(pin_is_exempt(Some(&policy), "child", "2.0.0"));
}

#[test]
fn a_version_qualified_exclusion_exempts_only_the_versions_it_names() {
    // Excluding child@1.0.0 says nothing about child@2.0.0, so a candidate
    // pinning the latter is still judged on its age.
    let policy = exclude(&["child@1.0.0"]);

    assert!(pin_is_exempt(Some(&policy), "child", "1.0.0"));
    assert!(!pin_is_exempt(Some(&policy), "child", "2.0.0"));
}

#[test]
fn a_package_the_policy_does_not_name_is_never_exempt() {
    let policy = exclude(&["other"]);

    assert!(!pin_is_exempt(Some(&policy), "child", "1.0.0"));
    assert!(!pin_is_exempt(None, "child", "1.0.0"));
}
