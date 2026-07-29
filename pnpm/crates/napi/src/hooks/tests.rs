use std::sync::Arc;

use serde_json::json;

use super::IgnoredDependenciesHook;

fn hook(names: &[&str]) -> IgnoredDependenciesHook {
    IgnoredDependenciesHook::new(None, names.iter().map(ToString::to_string).collect())
}

#[test]
fn strip_removes_ignored_names_from_dependencies_and_peer_dependencies() {
    let manifest = Arc::new(json!({
        "name": "consumer",
        "dependencies": {
            "@teambit/legacy": "^1.0.0",
            "left-pad": "^1.3.0",
        },
        "peerDependencies": {
            "@teambit/harmony": "^0.4.0",
            "react": "^18.0.0",
        },
    }));
    let stripped = hook(&["@teambit/legacy", "@teambit/harmony"]).strip(manifest);
    assert_eq!(
        *stripped,
        json!({
            "name": "consumer",
            "dependencies": { "left-pad": "^1.3.0" },
            "peerDependencies": { "react": "^18.0.0" },
        }),
    );
}

#[test]
fn strip_keeps_link_ranges_in_dependencies() {
    let manifest = Arc::new(json!({
        "dependencies": { "@teambit/legacy": "link:../legacy" },
        "peerDependencies": { "@teambit/legacy": "^1.0.0" },
    }));
    let stripped = hook(&["@teambit/legacy"]).strip(manifest);
    assert_eq!(
        *stripped,
        json!({
            "dependencies": { "@teambit/legacy": "link:../legacy" },
            "peerDependencies": {},
        }),
    );
}

#[test]
fn strip_shares_the_input_when_nothing_matches() {
    let manifest = Arc::new(json!({
        "dependencies": { "left-pad": "^1.3.0", "kept": "link:../kept" },
        "peerDependencies": { "react": "^18.0.0" },
    }));
    let stripped = hook(&["@teambit/legacy", "kept"]).strip(Arc::clone(&manifest));
    assert!(Arc::ptr_eq(&manifest, &stripped));
}
