use pnpm_lockfile::{DirectoryResolution, LockfileResolution};
use pnpm_resolving_resolver_base::{PkgResolutionId, ResolveResult};

use super::{super::test_support::manifest_result, extract_children};

#[test]
fn dependency_engines_runtime_is_walked_as_a_runtime_dependency() {
    let result = manifest_result(serde_json::json!({
        "name": "parent",
        "version": "1.0.0",
        "engines": {
            "runtime": {
                "name": "node",
                "version": "22.19.0",
                "onFail": "download",
            },
        },
    }));
    assert_eq!(
        extract_children(&result).unwrap(),
        vec![("node".to_string(), "runtime:22.19.0".to_string(), false, false)],
    );
}

#[test]
fn child_order_does_not_depend_on_manifest_property_order() {
    let forward = manifest_result(
        serde_json::from_str(
            r#"{
            "name": "parent",
            "version": "1.0.0",
            "dependencies": { "alpha": "1.0.0", "zulu": "2.0.0" }
        }"#,
        )
        .unwrap(),
    );
    let reverse = manifest_result(
        serde_json::from_str(
            r#"{
            "name": "parent",
            "version": "1.0.0",
            "dependencies": { "zulu": "2.0.0", "alpha": "1.0.0" }
        }"#,
        )
        .unwrap(),
    );

    let forward_children = extract_children(&forward).unwrap();
    let reverse_children = extract_children(&reverse).unwrap();
    dbg!(&forward_children, &reverse_children);
    assert_eq!(forward_children, reverse_children);
}

// Regression test for pnpm/pnpm#13334: npm ships bundled dependencies
// inside the package's own tarball, so they must not be resolved as
// edges of their own.
#[test]
fn bundled_dependencies_are_not_walked() {
    let result = manifest_result(serde_json::json!({
        "name": "parent",
        "version": "1.0.0",
        "dependencies": { "bundled-dep": "^1.0.0", "regular-dep": "^2.0.0" },
        "optionalDependencies": { "bundled-optional": "^3.0.0" },
        "bundledDependencies": ["bundled-dep", "bundled-optional"],
    }));
    assert_eq!(
        extract_children(&result).unwrap(),
        vec![("regular-dep".to_string(), "^2.0.0".to_string(), false, false)],
    );
}

#[test]
fn bundle_dependencies_spelling_is_honored() {
    let result = manifest_result(serde_json::json!({
        "name": "parent",
        "version": "1.0.0",
        "dependencies": { "bundled-dep": "^1.0.0", "regular-dep": "^2.0.0" },
        "bundleDependencies": ["bundled-dep"],
    }));
    assert_eq!(
        extract_children(&result).unwrap(),
        vec![("regular-dep".to_string(), "^2.0.0".to_string(), false, false)],
    );
}

#[test]
fn bundled_dependencies_true_bundles_every_dependency() {
    let result = manifest_result(serde_json::json!({
        "name": "parent",
        "version": "1.0.0",
        "dependencies": { "one": "^1.0.0", "two": "^2.0.0" },
        "optionalDependencies": { "three": "^3.0.0" },
        "bundledDependencies": true,
    }));
    assert_eq!(
        extract_children(&result).unwrap(),
        vec![("three".to_string(), "^3.0.0".to_string(), true, false)],
    );
}

// `bundledDependencies: true` names the `dependencies` keys, and upstream
// filters the merged `{...optionalDependencies, ...dependencies}` map, so an
// alias listed in both groups is dropped from both.
#[test]
fn bundled_dependencies_true_also_drops_the_optional_duplicate() {
    let result = manifest_result(serde_json::json!({
        "name": "parent",
        "version": "1.0.0",
        "dependencies": { "both": "^1.0.0" },
        "optionalDependencies": { "both": "^1.0.0", "optional-only": "^3.0.0" },
        "bundledDependencies": true,
    }));
    assert_eq!(
        extract_children(&result).unwrap(),
        vec![("optional-only".to_string(), "^3.0.0".to_string(), true, false)],
    );
}

#[test]
fn dependencies_meta_marks_children_as_injected() {
    let result = manifest_result(serde_json::json!({
        "name": "parent",
        "version": "1.0.0",
        "dependencies": { "injected": "workspace:*", "linked": "workspace:*" },
        "dependenciesMeta": { "injected": { "injected": true } },
    }));
    assert_eq!(
        extract_children(&result).unwrap(),
        vec![
            ("injected".to_string(), "workspace:*".to_string(), false, true),
            ("linked".to_string(), "workspace:*".to_string(), false, false),
        ],
    );
}

/// Suppressing the warning beats emitting a malformed `name@` payload.
#[test]
fn deprecated_pkg_name_ver_falls_back_to_the_manifest() {
    let result = |manifest: serde_json::Value| ResolveResult {
        id: PkgResolutionId::from("git-pkg@https://example.com/repo.tgz"),
        name_ver: None,
        latest: None,
        published_at: None,
        manifest: Some(std::sync::Arc::new(manifest)),
        resolution: LockfileResolution::Directory(DirectoryResolution {
            directory: ".".to_string(),
        }),
        resolved_via: "git-repository".to_string(),
        normalized_bare_specifier: None,
        alias: Some("git-pkg".to_string()),
        policy_violation: None,
    };

    assert_eq!(
        super::deprecated_pkg_name_ver(&result(
            serde_json::json!({ "name": "git-pkg", "version": "2.0.0" })
        )),
        Some(("git-pkg".to_string(), "2.0.0".to_string())),
    );
    assert_eq!(
        super::deprecated_pkg_name_ver(&result(serde_json::json!({ "name": "git-pkg" }))),
        None,
    );
}
