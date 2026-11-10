use pnpm_lockfile::{DirectoryResolution, LockfileResolution};
use pnpm_resolving_resolver_base::{PkgResolutionId, ResolveResult};

use super::{
    super::test_support::manifest_result, DependencySpec, extract_children,
    extract_peer_dependencies,
};
use crate::resolved_tree::PeerDep;
use pnpm_catalogs_types::Catalogs;
use rustc_hash::FxHashSet as HashSet;
use std::path::Path;

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
        vec![DependencySpec {
            alias: "node".to_string(),
            bare_specifier: "runtime:22.19.0".to_string(),
            optional: false,
            injected: false
        }],
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
        vec![DependencySpec {
            alias: "regular-dep".to_string(),
            bare_specifier: "^2.0.0".to_string(),
            optional: false,
            injected: false
        }],
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
        vec![DependencySpec {
            alias: "regular-dep".to_string(),
            bare_specifier: "^2.0.0".to_string(),
            optional: false,
            injected: false
        }],
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
        vec![DependencySpec {
            alias: "three".to_string(),
            bare_specifier: "^3.0.0".to_string(),
            optional: true,
            injected: false
        }],
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
        vec![DependencySpec {
            alias: "optional-only".to_string(),
            bare_specifier: "^3.0.0".to_string(),
            optional: true,
            injected: false
        }],
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
            DependencySpec {
                alias: "injected".to_string(),
                bare_specifier: "workspace:*".to_string(),
                optional: false,
                injected: true
            },
            DependencySpec {
                alias: "linked".to_string(),
                bare_specifier: "workspace:*".to_string(),
                optional: false,
                injected: false
            },
        ],
    );
}

/// Suppressing the warning beats emitting a malformed `name@` payload.
#[test]
fn deprecated_pkg_name_ver_falls_back_to_the_manifest() {
    let result = |manifest: serde_json::Value| ResolveResult {
        id: PkgResolutionId::from("git-pkg@https://example.com/repo.tgz"),
        resolution: LockfileResolution::Directory(DirectoryResolution {
            directory: ".".to_string(),
        }),
        resolved_via: "git-repository".to_string(),
        normalized_bare_specifier: None,
        alias: Some("git-pkg".to_string()),
        policy_violation: None,
        package: pnpm_resolving_resolver_base::ResolvedPackageInfo {
            name_ver: None,
            latest: None,
            published_at: None,
            manifest: Some(std::sync::Arc::new(manifest)),
            non_deprecated_alternative: None,
        },
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

#[test]
fn only_taken_peers_are_dereferenced_from_catalogs() {
    let result = manifest_result(serde_json::json!({
        "name": "lib",
        "version": "1.0.0",
        "dependencies": { "foo": "catalog:" },
        "peerDependencies": { "foo": "catalog:", "bar": "catalog:" },
    }));
    let catalogs = Catalogs::from([(
        "default".to_string(),
        std::collections::BTreeMap::from([
            ("foo".to_string(), "^1.0.0".to_string()),
            ("bar".to_string(), "^2.0.0".to_string()),
        ]),
    )]);
    let peer = |version: &str| PeerDep { version: version.to_string(), optional: false };

    let peer_set = extract_peer_dependencies(
        &result,
        &HashSet::default(),
        Some(&catalogs),
        None,
        Path::new("."),
    )
    .unwrap();

    assert_eq!(
        peer_set
            .declared()
            .iter()
            .collect::<Vec<_>>(),
        [(&"bar".to_string(), &peer("catalog:")), (&"foo".to_string(), &peer("catalog:"))],
        "the lockfile records the peers as the manifest writes them",
    );
    assert_eq!(
        peer_set
            .taken()
            .iter()
            .collect::<Vec<_>>(),
        [(&"bar".to_string(), &peer("^2.0.0"))],
        "the unshadowed `foo` dependency supplies the package itself",
    );
}

#[test]
fn directory_specifiers_record_the_resolvers_effective_groups() {
    let manifest = serde_json::json!({
        "dependencies": { "regular": "catalog:", "both": "^1", "bundled": "*" },
        "optionalDependencies": { "both": "^2", "optional": "file:../optional" },
        "devDependencies": { "development": "*" },
        "peerDependencies": { "peer": "workspace:*" },
        "bundledDependencies": ["bundled"],
        "dependenciesMeta": { "regular": { "injected": true } }
    });
    let recorded = super::declared_specifiers(&manifest, "directory").expect("read specifiers");
    assert_eq!(
        serde_json::to_value(recorded).expect("serialize specifiers"),
        serde_json::json!({
            "dependencies": { "regular": "catalog:" },
            "optionalDependencies": { "both": "^1", "optional": "file:../optional" },
            "dependenciesMeta": { "regular": { "injected": true } }
        })
    );
}

#[test]
fn directory_specifiers_distinguish_empty_records_and_manifest_changes() {
    let empty = super::declared_specifiers(&serde_json::json!({}), "directory")
        .expect("read empty specifiers");
    assert_eq!(
        serde_json::to_value(&empty).expect("serialize empty specifiers"),
        serde_json::json!({})
    );
    for field in ["dependencies", "optionalDependencies", "dependenciesMeta"] {
        let value = if field == "dependenciesMeta" {
            serde_json::json!({ "child": { "injected": true } })
        } else {
            serde_json::json!({ "child": "workspace:../child" })
        };
        let manifest = serde_json::json!({ field: value });
        assert_ne!(
            super::declared_specifiers(&manifest, "directory").expect("read changed specifiers"),
            empty
        );
    }
}

#[test]
fn directory_specifiers_preserve_every_dependency_source() {
    for specifier in [
        "^1.0.0",
        "catalog:",
        "catalog:tools",
        "workspace:*",
        "workspace:../host",
        "file:../local",
        "link:../linked",
        "file:../archive.tgz",
        "npm:real-name@^1",
        "https://example.invalid/archive.tgz",
        "github:example/project#abcdef",
    ] {
        for group in ["dependencies", "optionalDependencies"] {
            let manifest = serde_json::json!({ group: { "child": specifier } });
            let record =
                super::declared_specifiers(&manifest, "directory").expect("extract inputs");
            assert_eq!(serde_json::to_value(record).expect("serialize inputs"), manifest);
        }
    }
}

#[test]
fn directory_specifiers_include_runtime_dependencies() {
    let manifest = serde_json::json!({
        "engines": { "runtime": { "name": "node", "version": "22.19.0", "onFail": "download" } }
    });
    let record = super::declared_specifiers(&manifest, "directory").expect("extract runtime input");
    assert_eq!(
        serde_json::to_value(record).expect("serialize inputs"),
        serde_json::json!({
            "dependencies": { "node": "runtime:22.19.0" }
        })
    );
}
