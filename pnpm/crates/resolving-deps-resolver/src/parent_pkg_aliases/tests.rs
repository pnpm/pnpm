use std::collections::HashSet;

use super::{ParentPkgAliases, peer_shadowed_dependencies};

fn names<const COUNT: usize>(names: [&str; COUNT]) -> HashSet<String> {
    names.into_iter().map(str::to_string).collect()
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
