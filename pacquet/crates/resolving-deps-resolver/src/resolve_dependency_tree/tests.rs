use pacquet_lockfile::PkgNameVerPeer;

use super::landed_on_prior_entry;

fn key(raw: &str) -> PkgNameVerPeer {
    raw.parse().expect("parse snapshot key")
}

#[test]
fn matches_a_plain_registry_id() {
    assert!(landed_on_prior_entry(&key("foo@1.0.0"), "foo@1.0.0"));
    assert!(!landed_on_prior_entry(&key("foo@1.0.0"), "foo@1.1.0"));
}

#[test]
fn strips_the_recorded_key_peer_and_patch_suffixes() {
    assert!(landed_on_prior_entry(&key("foo@1.0.0(bar@2.0.0)"), "foo@1.0.0"));
    assert!(landed_on_prior_entry(&key("foo@1.0.0(patch_hash=0000)"), "foo@1.0.0"));
    assert!(landed_on_prior_entry(&key("foo@1.0.0(patch_hash=0000)(bar@2.0.0)"), "foo@1.0.0"));
}

#[test]
fn strips_the_resolved_id_patch_suffix() {
    assert!(landed_on_prior_entry(
        &key("foo@1.0.0(patch_hash=0000)"),
        "foo@1.0.0(patch_hash=0000)"
    ));
    assert!(landed_on_prior_entry(&key("foo@1.0.0"), "foo@1.0.0(patch_hash=0000)"));
}

#[test]
fn matches_a_name_prefixed_file_id() {
    assert!(landed_on_prior_entry(&key("foo@file:packages/foo"), "foo@file:packages/foo"));
    assert!(!landed_on_prior_entry(&key("foo@file:packages/foo"), "file:packages/foo"));
}

#[test]
fn owner_missing_record_is_written_once_per_generation() {
    use super::{ChildrenOwner, WorkspaceTreeCtx, lock_recoverable};
    use std::collections::{HashMap, HashSet};

    let ctx = WorkspaceTreeCtx::default();
    let owner = ChildrenOwner {
        depth: 1,
        importer_order: 0,
        parent_path: vec!["root-dep@1.0.0".to_string()],
        importer_id: ".".to_string(),
    };
    lock_recoverable(&ctx.children_owner_by_id).insert("pkg@1.0.0".to_string(), owner.clone());

    let miss = |names: &[&str]| {
        let mut map: HashMap<String, HashSet<String>> = HashMap::new();
        map.insert("pkg@1.0.0".to_string(), names.iter().map(|name| (*name).to_string()).collect());
        map
    };

    ctx.record_first_walk_missing("pkg-a", &miss(&["peer"]));
    assert_eq!(
        ctx.first_walk_missing_by_pkg().get("pkg@1.0.0"),
        Some(&miss(&["peer"]).remove("pkg@1.0.0").unwrap()),
    );

    ctx.record_first_walk_missing(".", &miss(&["peer", "other-peer"]));
    let recorded = ctx.first_walk_missing_by_pkg();
    assert_eq!(recorded.get("pkg@1.0.0").map(HashSet::len), Some(2));

    ctx.record_first_walk_missing(".", &miss(&[]));
    let recorded = ctx.first_walk_missing_by_pkg();
    assert!(
        recorded.get("pkg@1.0.0").is_some_and(|names| names.contains("peer")),
        "the owner's post-hoist pass must not refresh the generation's record",
    );

    let new_owner = ChildrenOwner { depth: 0, ..owner };
    lock_recoverable(&ctx.children_owner_by_id).insert("pkg@1.0.0".to_string(), new_owner);
    ctx.record_first_walk_missing(".", &miss(&[]));
    assert_eq!(
        ctx.first_walk_missing_by_pkg().get("pkg@1.0.0").map(HashSet::len),
        Some(0),
        "a new ownership generation records afresh",
    );
}

mod higher_direct_dep_version {
    use std::collections::HashMap;

    use node_semver::{Range, Version};

    use super::super::{DirectDepVersions, higher_direct_dep_version};

    fn direct(name: &str, versions: &[&str]) -> DirectDepVersions {
        let parsed =
            versions.iter().map(|raw| raw.parse::<Version>().expect("parse version")).collect();
        HashMap::from([(name.to_string(), parsed)])
    }

    fn ver(raw: &str) -> Version {
        raw.parse().expect("parse version")
    }

    fn range(raw: &str) -> Range {
        raw.parse().expect("parse range")
    }

    #[test]
    fn picks_the_highest_in_range_version_above_the_pin() {
        let direct = direct("foo", &["1.1.0", "1.5.0", "2.0.0"]);
        assert_eq!(
            higher_direct_dep_version(Some(&direct), "foo", &ver("1.0.0"), &range("^1.0.0")),
            Some(ver("1.5.0")),
        );
    }

    #[test]
    fn none_when_no_direct_version_is_higher() {
        let direct = direct("foo", &["1.0.0"]);
        assert!(
            higher_direct_dep_version(Some(&direct), "foo", &ver("1.0.0"), &range("^1.0.0"))
                .is_none(),
        );
    }

    #[test]
    fn does_not_refresh_a_prerelease_onto_a_stable_range() {
        // Matches pnpm's `semver.satisfies(.., true)`: a prerelease does not
        // satisfy a range that doesn't admit prereleases, so no refresh.
        let direct = direct("foo", &["1.2.0-beta.1"]);
        assert!(
            higher_direct_dep_version(Some(&direct), "foo", &ver("1.0.0"), &range("^1.0.0"))
                .is_none(),
        );
    }

    #[test]
    fn refreshes_a_prerelease_when_the_range_admits_it() {
        let direct = direct("foo", &["1.2.0-beta.1"]);
        assert_eq!(
            higher_direct_dep_version(Some(&direct), "foo", &ver("1.0.0"), &range(">=1.2.0-0")),
            Some(ver("1.2.0-beta.1")),
        );
    }
}
