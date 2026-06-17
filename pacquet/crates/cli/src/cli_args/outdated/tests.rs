use super::{
    Change, OutdatedDependencyOptions, OutdatedPackage, classify, render_json, render_latest,
    sort_outdated,
};
use node_semver::Version;
use pacquet_package_manifest::DependencyGroup;

fn v(text: &str) -> Version {
    text.parse().expect("parse semver")
}

fn pkg(name: &str, current: &str, target: &str, group: DependencyGroup) -> OutdatedPackage {
    OutdatedPackage {
        alias: name.to_string(),
        package_name: name.to_string(),
        belongs_to: group,
        current: v(current),
        target: v(target),
        deprecated: None,
        homepage: None,
    }
}

#[test]
fn classify_detects_each_bump_kind() {
    assert_eq!(classify(&v("1.0.0"), &v("2.0.0")), Change::Breaking);
    assert_eq!(classify(&v("1.0.0"), &v("1.1.0")), Change::Feature);
    assert_eq!(classify(&v("1.0.0"), &v("1.0.1")), Change::Fix);
    assert_eq!(classify(&v("1.0.0"), &v("1.0.0")), Change::None);
    assert_eq!(classify(&v("1.0.0-alpha.1"), &v("1.0.0")), Change::Unknown);
}

#[test]
fn include_default_covers_all_three_groups() {
    let opts = OutdatedDependencyOptions { prod: false, dev: false, no_optional: false };
    assert_eq!(
        opts.include(),
        vec![DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
    );
}

#[test]
fn include_prod_keeps_dependencies_and_optional() {
    let opts = OutdatedDependencyOptions { prod: true, dev: false, no_optional: false };
    assert_eq!(opts.include(), vec![DependencyGroup::Prod, DependencyGroup::Optional]);
}

#[test]
fn include_dev_keeps_only_dev() {
    let opts = OutdatedDependencyOptions { prod: false, dev: true, no_optional: false };
    assert_eq!(opts.include(), vec![DependencyGroup::Dev]);
}

#[test]
fn include_no_optional_drops_optional() {
    let opts = OutdatedDependencyOptions { prod: false, dev: false, no_optional: true };
    assert_eq!(opts.include(), vec![DependencyGroup::Prod, DependencyGroup::Dev]);
}

#[test]
fn default_sort_orders_by_change_then_name() {
    let mut outdated = vec![
        pkg("breaking-z", "1.0.0", "2.0.0", DependencyGroup::Prod),
        pkg("fix-a", "1.0.0", "1.0.1", DependencyGroup::Prod),
        pkg("fix-b", "1.0.0", "1.0.1", DependencyGroup::Prod),
        pkg("feature-a", "1.0.0", "1.1.0", DependencyGroup::Prod),
    ];
    sort_outdated(&mut outdated, None);
    let order: Vec<&str> = outdated.iter().map(|item| item.package_name.as_str()).collect();
    assert_eq!(order, vec!["fix-a", "fix-b", "feature-a", "breaking-z"]);
}

#[test]
fn json_report_has_expected_shape() {
    let outdated = vec![pkg("foo", "1.0.0", "2.0.0", DependencyGroup::Dev)];
    let value: serde_json::Value =
        serde_json::from_str(&render_json(&outdated, false)).expect("valid JSON");
    let entry = &value["foo"];
    assert_eq!(entry["current"], "1.0.0");
    assert_eq!(entry["latest"], "2.0.0");
    assert_eq!(entry["wanted"], "1.0.0");
    assert_eq!(entry["isDeprecated"], false);
    assert_eq!(entry["dependencyType"], "devDependencies");
    assert!(entry.get("latestManifest").is_none(), "latestManifest is --long only");
}

// Ports `deps/inspection/commands/test/outdated/renderLatest.test.ts`.
// Colors are emitted only on a TTY, so the captured (non-TTY) output is
// plain text here.
#[test]
fn render_latest_outdated_and_deprecated() {
    let mut item = pkg("foo", "0.0.1", "1.0.0", DependencyGroup::Prod);
    item.deprecated = Some("This package is deprecated".to_string());
    let output = render_latest(&item);
    assert!(output.contains("1.0.0"), "shows the latest version: {output}");
    assert!(output.contains("(deprecated)"), "flags the deprecation: {output}");
}

#[test]
fn render_latest_outdated_and_not_deprecated() {
    let item = pkg("foo", "0.0.1", "1.0.0", DependencyGroup::Prod);
    let output = render_latest(&item);
    assert!(output.contains("1.0.0"), "shows the latest version: {output}");
    assert!(!output.contains("(deprecated)"), "no deprecation marker: {output}");
}

#[test]
fn json_report_long_includes_latest_manifest() {
    let mut item = pkg("foo", "1.0.0", "2.0.0", DependencyGroup::Prod);
    item.deprecated = Some("do not use".to_string());
    item.homepage = Some("https://example.com".to_string());
    let value: serde_json::Value =
        serde_json::from_str(&render_json(&[item], true)).expect("valid JSON");
    let manifest = &value["foo"]["latestManifest"];
    assert_eq!(manifest["version"], "2.0.0");
    assert_eq!(manifest["deprecated"], "do not use");
    assert_eq!(manifest["homepage"], "https://example.com");
    assert_eq!(value["foo"]["isDeprecated"], true);
}
