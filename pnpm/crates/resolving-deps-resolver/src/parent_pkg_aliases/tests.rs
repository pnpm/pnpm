use super::{
    ParentPkgAliases, declared_peer_dependencies, own_dependency_names, peer_shadowed_dependencies,
};
use crate::resolved_tree::PeerDep;
use rustc_hash::FxHashSet as HashSet;

fn names<const COUNT: usize>(names: [&str; COUNT]) -> HashSet<String> {
    names
        .into_iter()
        .map(str::to_string)
        .collect()
}

#[test]
fn a_level_sees_every_alias_above_it() {
    let root = ParentPkgAliases::root(names(["root-dep"]));
    let level1 = root.extend(names(["child"]));
    let level2 = level1.extend(names(["grandchild"]));

    assert!(level2.contains("root-dep"));
    assert!(level2.contains("child"));
    assert!(level2.contains("grandchild"));
    assert!(!level2.contains("unrelated"));
    assert!(!root.contains("child"), "a level's aliases stay out of the scopes above it");
}

#[test]
fn only_in_scope_peers_shadow_the_own_dependency() {
    let manifest = serde_json::json!({
        "dependencies": { "in-scope": "^1.0.0", "out-of-scope": "^1.0.0", "not-a-peer": "^1.0.0" },
        "peerDependencies": { "in-scope": "*", "out-of-scope": "*" },
    });
    let scope = ParentPkgAliases::root(names(["in-scope"]));

    assert_eq!(peer_shadowed_dependencies(Some(&manifest), &scope, false), names(["in-scope"]));
}

#[test]
fn auto_install_peers_shadows_every_own_dependency() {
    let manifest = serde_json::json!({
        "dependencies": { "in-scope": "^1.0.0", "out-of-scope": "^1.0.0" },
        "peerDependencies": { "in-scope": "*", "out-of-scope": "*" },
    });
    let scope = ParentPkgAliases::root(names(["in-scope"]));

    assert_eq!(
        peer_shadowed_dependencies(Some(&manifest), &scope, true),
        names(["in-scope", "out-of-scope"]),
    );
}

#[test]
fn a_manifest_without_both_sections_shadows_nothing() {
    let scope = ParentPkgAliases::root(names(["dep"]));
    let peers_only = serde_json::json!({ "peerDependencies": { "dep": "*" } });
    let deps_only = serde_json::json!({ "dependencies": { "dep": "^1.0.0" } });

    assert!(peer_shadowed_dependencies(None, &scope, true).is_empty());
    assert!(peer_shadowed_dependencies(Some(&peers_only), &scope, true).is_empty());
    assert!(peer_shadowed_dependencies(Some(&deps_only), &scope, true).is_empty());
}

#[test]
fn auto_install_peers_keeps_an_out_of_scope_optional_peer_as_a_dependency() {
    let manifest = serde_json::json!({
        "dependencies": { "in-scope": "^1.0.0", "out-of-scope": "^1.0.0" },
        "peerDependencies": { "in-scope": "*", "out-of-scope": "*" },
        "peerDependenciesMeta": {
            "in-scope": { "optional": true },
            "out-of-scope": { "optional": true },
        },
    });
    let scope = ParentPkgAliases::root(names(["in-scope"]));

    assert_eq!(peer_shadowed_dependencies(Some(&manifest), &scope, true), names(["in-scope"]));
}

#[test]
fn own_dependencies_are_the_unshadowed_ones_plus_optional_deps_and_the_name() {
    let manifest = serde_json::json!({
        "name": "pkg",
        "dependencies": { "shadowed": "^1.0.0", "kept": "^1.0.0", "also-optional": "^1.0.0" },
        "optionalDependencies": { "also-optional": "^1.0.0" },
    });
    let shadowed = names(["shadowed", "also-optional"]);

    let own: HashSet<&str> =
        own_dependency_names(&manifest, |name| shadowed.contains(name)).collect();

    assert_eq!(own, HashSet::from_iter(["kept", "also-optional", "pkg"]));
}

#[test]
fn declared_peers_keep_a_same_named_dependency_and_drop_optional_deps_and_the_name() {
    let manifest = serde_json::json!({
        "name": "pkg",
        "dependencies": { "dep": "1.0.0" },
        "optionalDependencies": { "optional": "1.0.0" },
        "peerDependencies": { "dep": "^1.0.0", "optional": "^1.0.0", "pkg": "*", "peer": "^2.0.0" },
        "peerDependenciesMeta": { "dep": { "optional": true }, "meta-only": { "optional": true } },
    });

    let declared: Vec<(String, PeerDep)> =
        declared_peer_dependencies(&manifest).into_iter().collect();

    assert_eq!(
        declared,
        [
            ("dep".to_string(), PeerDep { version: "^1.0.0".to_string(), optional: true }),
            ("meta-only".to_string(), PeerDep { version: "*".to_string(), optional: true }),
            ("peer".to_string(), PeerDep { version: "^2.0.0".to_string(), optional: false }),
        ],
    );
}
