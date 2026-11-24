use super::{
    DeprecatedPackage,
    InstalledPackage,
    deprecation,
    render_deprecations,
};
use pnpm_resolving_resolver_base::{
    NonDeprecatedAlternative,
    ResolvedPackageInfo,
};
use pretty_assertions::assert_eq;
use std::sync::Arc;

fn resolved(manifest: serde_json::Value, alternative: Option<&str>) -> ResolvedPackageInfo {
    ResolvedPackageInfo {
        manifest: Some(Arc::new(manifest)),
        non_deprecated_alternative: alternative.map(|version| NonDeprecatedAlternative {
            version: version.to_string(),
            outside_declared_range: false,
        }),
        ..ResolvedPackageInfo::default()
    }
}

fn installed(name: &str, version: &str) -> InstalledPackage {
    InstalledPackage {
        name: name.to_string(),
        version: version.to_string(),
        specifier: version.to_string(),
    }
}

fn package(name: &str, projects: &[&str], via: &[&str]) -> DeprecatedPackage {
    DeprecatedPackage {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        deprecated: format!("{name} is no longer maintained"),
        non_deprecated_version: None,
        projects: projects
            .iter()
            .map(ToString::to_string)
            .collect(),
        via: via
            .iter()
            .map(ToString::to_string)
            .collect(),
    }
}

#[test]
fn reads_the_deprecation_message_and_alternative() {
    let found = deprecation(
        installed("request", "2.88.2"),
        &resolved(serde_json::json!({ "deprecated": "see\nhttps://example.com" }), Some("3.0.0")),
    )
    .expect("deprecated");
    assert_eq!(found.deprecated, "see https://example.com");
    assert_eq!(found.non_deprecated_version.as_deref(), Some("3.0.0"));
}

#[test]
fn a_package_without_a_deprecation_message_is_not_deprecated() {
    for manifest in [
        serde_json::json!({}),
        serde_json::json!({ "deprecated": "" }),
        serde_json::json!({ "deprecated": false }),
    ] {
        assert!(deprecation(installed("ms", "2.1.3"), &resolved(manifest, None)).is_none());
    }
}

#[test]
fn renders_projects_and_the_chain_of_a_subdependency() {
    let mut request = package("request", &[".", "packages/a", "packages/b", "packages/c"], &[]);
    request.non_deprecated_version = Some("3.0.0".to_string());
    let output = render_deprecations(&[request, package("uuid", &[], &["request", "har"])]);
    assert_eq!(
        output,
        "request 1.0.0  ., packages/a, packages/b and 1 more\n  \
         request is no longer maintained\n  \
         3.0.0 is not deprecated\n\
         uuid 1.0.0  via request > har\n  \
         uuid is no longer maintained\n\n\
         2 deprecated packages",
    );
}

#[test]
fn renders_a_clean_result() {
    assert_eq!(render_deprecations(&[]), "No deprecated packages found");
}
