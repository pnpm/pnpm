use std::collections::{
    HashMap,
    HashSet,
};

use super::{
    CombinationSearch,
    PeerGroups,
    search::MAX_COMBINATIONS,
};
use crate::{
    BoundProviders,
    LockfileEntries,
    PackageMetadata,
    PkgNameVerPeer,
    SnapshotDepRef,
    SnapshotEntry,
};

#[test]
fn expanded_candidates_stop_at_the_search_budget() {
    let candidates: Vec<_> = (0..12)
        .map(|index| {
            vec![
                format!("provider-{index}@1.0.0(nested@1.0.0)").parse().unwrap(),
                format!("provider-{index}@1.0.0").parse().unwrap(),
            ]
        })
        .collect();
    let mut search = CombinationSearch::new(PeerGroups::Full, "unmatched");
    assert_eq!(search.remaining, MAX_COMBINATIONS);
    assert_eq!(search.find(&candidates), None);
    assert_eq!(search.remaining, 0);
    assert_eq!(search.find(&candidates), None);
}

#[test]
fn complete_provider_set_is_checked_before_omissions_exhaust_the_budget() {
    let names: Vec<_> = (0..13)
        .map(|index| format!("provider-{index:02}@1.0.0"))
        .collect();
    let candidates: Vec<_> = names
        .iter()
        .map(|name| vec![name.parse().unwrap()])
        .collect();
    let hash = pnpm_crypto_hash::create_short_hash(&names.join(")("));
    let snapshots = HashMap::new();
    let consumer = "consumer@1.0.0".parse().unwrap();
    let mut cycle_probe = super::CycleProbe::new(&snapshots, &consumer);
    let recovered = super::match_candidates(&candidates, None, &hash, &mut cycle_probe).unwrap();
    assert_eq!(recovered.len(), names.len());
    for name in names {
        assert!(recovered.contains(&name.parse().unwrap()));
    }
}

#[test]
fn large_peer_sets_do_not_use_the_call_stack() {
    let candidates: Vec<_> = (0..10_000)
        .map(|index| vec![format!("provider-{index}@1.0.0").parse().unwrap()])
        .collect();
    let mut search = CombinationSearch::new(PeerGroups::Full, "unmatched");
    search.remaining = 2;
    assert_eq!(search.find(&candidates), None);
    assert_eq!(search.remaining, 0);
}

#[test]
fn deduplicates_collapsing_bare_candidates_while_keeping_distinct_providers() {
    let mut candidates = vec![vec![
        "slot0-first@1.0.0".parse::<PkgNameVerPeer>().unwrap(),
        "slot0-second@1.0.0".parse::<PkgNameVerPeer>().unwrap(),
    ]];
    let mut expected_segments = vec!["slot0-second@1.0.0".to_string()];
    for idx in 0..13 {
        candidates.push(vec![
            format!("dup-{idx:02}@1.0.0(peer-a@1.0.0)").parse().unwrap(),
            format!("dup-{idx:02}@1.0.0(peer-b@1.0.0)").parse().unwrap(),
        ]);
        expected_segments.push(format!("dup-{idx:02}@1.0.0"));
    }
    expected_segments.sort_unstable();
    let hash = pnpm_crypto_hash::create_short_hash(&expected_segments.join(")("));
    let mut bare_search = CombinationSearch::new(PeerGroups::Bare, &hash);
    let recovered = bare_search.find(&candidates).expect("recover second branch after dedup");
    assert_eq!(bare_search.remaining, MAX_COMBINATIONS - 2);
    assert!(recovered.contains(&"slot0-second@1.0.0".parse().unwrap()));

    let first_file: PkgNameVerPeer = "same@file:packages/a".parse().unwrap();
    let second_file: PkgNameVerPeer = "same@file:packages/b".parse().unwrap();
    let mut packages = HashMap::new();
    for key in [&first_file, &second_file] {
        packages.insert(key.clone(), pkg_meta_with_version("1.0.0"));
    }
    let pair_hash = pnpm_crypto_hash::create_short_hash("same@1.0.0");
    let mut pair_search = CombinationSearch::new(PeerGroups::Pair(&packages), &pair_hash);
    let pair_candidates = vec![vec![first_file.clone(), second_file]];
    assert_eq!(pair_search.find(&pair_candidates), Some(HashSet::from([first_file])));
}

#[test]
fn descendant_walk_indexes_active_peers_and_charges_link_refs() {
    let target_peer: PkgNameVerPeer = "trans-0999@1.0.0".parse().unwrap();
    let hash = pnpm_crypto_hash::create_short_hash(&target_peer.to_string());
    let parent_key: PkgNameVerPeer = format!("parent@1.0.0({hash})").parse().unwrap();

    let mut packages = HashMap::new();
    for key in [
        parent_key.without_peer(),
        "mid@1.0.0".parse().unwrap(),
        "carrier@1.0.0".parse().unwrap(),
        "noisy@1.0.0".parse().unwrap(),
        target_peer.clone(),
    ] {
        packages.insert(key, empty_pkg_meta());
    }

    let mut snapshots = HashMap::new();
    let mut parent_deps = HashMap::new();
    for idx in 0..2_000 {
        let leaf_key: PkgNameVerPeer = format!("leaf-{idx:04}@1.0.0").parse().unwrap();
        packages.insert(leaf_key.clone(), empty_pkg_meta());
        snapshots.insert(leaf_key, SnapshotEntry::default());
        parent_deps.insert(
            format!("leaf-{idx:04}").parse().unwrap(),
            SnapshotDepRef::Plain("1.0.0".parse().unwrap()),
        );
    }
    parent_deps.insert("noisy".parse().unwrap(), SnapshotDepRef::Plain("1.0.0".parse().unwrap()));
    parent_deps.insert("mid".parse().unwrap(), SnapshotDepRef::Plain("1.0.0".parse().unwrap()));

    snapshots.insert(
        "noisy@1.0.0".parse().unwrap(),
        SnapshotEntry {
            transitive_peer_dependencies: Some(
                (0..1_000)
                    .map(|idx| format!("unrelated-{idx:04}"))
                    .collect(),
            ),
            ..SnapshotEntry::default()
        },
    );
    snapshots.insert(
        "mid@1.0.0".parse().unwrap(),
        SnapshotEntry {
            dependencies: Some(HashMap::from([(
                "carrier".parse().unwrap(),
                SnapshotDepRef::Plain("1.0.0".parse().unwrap()),
            )])),
            transitive_peer_dependencies: Some(vec!["trans-0999".to_string()]),
            ..SnapshotEntry::default()
        },
    );
    snapshots.insert(
        "carrier@1.0.0".parse().unwrap(),
        SnapshotEntry {
            dependencies: Some(HashMap::from([(
                "trans-0999".parse().unwrap(),
                SnapshotDepRef::Plain("1.0.0".parse().unwrap()),
            )])),
            ..SnapshotEntry::default()
        },
    );
    snapshots.insert(target_peer.clone(), SnapshotEntry::default());
    snapshots.insert(
        parent_key.clone(),
        SnapshotEntry {
            dependencies: Some(parent_deps),
            transitive_peer_dependencies: Some(
                (0..1_000)
                    .map(|idx| format!("trans-{idx:04}"))
                    .collect(),
            ),
            ..SnapshotEntry::default()
        },
    );

    let entries = LockfileEntries { packages: Some(&packages), snapshots: Some(&snapshots) };
    let recovered = BoundProviders::of_snapshot(entries, &parent_key);
    assert_eq!(recovered.provider_keys(&target_peer), Some(&HashSet::from([target_peer.clone()])));

    let mut flooded_deps =
        HashMap::from([("mid".parse().unwrap(), SnapshotDepRef::Plain("1.0.0".parse().unwrap()))]);
    for idx in 0..65_537 {
        flooded_deps.insert(
            format!("ws-link-{idx:05}").parse().unwrap(),
            SnapshotDepRef::Link("packages/ws".to_string()),
        );
    }
    snapshots.insert(
        parent_key.clone(),
        SnapshotEntry {
            dependencies: Some(flooded_deps),
            transitive_peer_dependencies: Some(vec!["trans-0999".to_string()]),
            ..SnapshotEntry::default()
        },
    );
    let entries_flooded =
        LockfileEntries { packages: Some(&packages), snapshots: Some(&snapshots) };
    assert!(BoundProviders::of_snapshot(entries_flooded, &parent_key).is_hashed());
}

fn empty_pkg_meta() -> PackageMetadata {
    serde_saphyr::from_str("resolution: { type: directory, directory: pkg }").unwrap()
}

fn pkg_meta_with_version(version: &str) -> PackageMetadata {
    serde_saphyr::from_str(&format!(
        "resolution: {{ type: directory, directory: pkg }}\nversion: {version}\n",
    ))
    .unwrap()
}
