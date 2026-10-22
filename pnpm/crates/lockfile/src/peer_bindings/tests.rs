use super::{BoundProviders, PeerBindings};
use crate::{Lockfile, PkgNameVerPeer};
use pretty_assertions::assert_eq;
use std::path::{Path, PathBuf};

fn key(text: &str) -> PkgNameVerPeer {
    text.parse().expect("parse key")
}

#[test]
fn bound_providers_match_targets_by_name_and_version_and_accept_all_for_hashed() {
    let explicit = BoundProviders::from_key(&key("a@1.0.0(b@2.0.0(c@1.0.0))"));
    assert!(!explicit.is_hashed());
    assert!(explicit.binds(&key("b@2.0.0")));
    assert!(explicit.binds(&key("b@2.0.0(other@1.0.0)")));
    assert!(!explicit.binds(&key("b@1.0.0")));
    assert!(!explicit.binds(&key("c@1.0.0")));

    let none = BoundProviders::from_key(&key("a@1.0.0"));
    assert!(!none.is_hashed());
    assert!(!none.binds(&key("b@2.0.0")));

    let hashed = BoundProviders::from_key(&key("a@1.0.0(0123456789abcdef0123456789abcdef)"));
    assert!(hashed.is_hashed());
    assert!(hashed.binds(&key("b@2.0.0")));
    assert!(hashed.binds(&key("anything@9.9.9")));
}

#[test]
fn bound_providers_of_snapshot_disambiguates_bound_peer_from_own_dependency_under_a_hashed_suffix()
{
    let suffix = PeerBindings::Explicit(vec!["bound@1.0.0".to_string()])
        .to_suffix(3)
        .expect("hashed suffix");
    let pkg_key = key(&format!("pkg@1.0.0{suffix}"));
    let yaml = format!(
        r"lockfileVersion: '9.0'
packages:
  pkg@1.0.0:
    resolution: {{integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==}}
    peerDependencies:
      bound: ^1.0.0
      own: ^1.0.0
snapshots:
  '{pkg_key}':
    dependencies:
      bound: 1.0.0
      own: 2.0.0
",
    );
    let lockfile: Lockfile = serde_saphyr::from_str(&yaml).expect("parse lockfile");
    let providers = BoundProviders::of_snapshot(
        crate::LockfileEntries {
            packages: lockfile.packages.as_ref(),
            snapshots: lockfile.snapshots.as_ref(),
        },
        &pkg_key,
    );
    assert!(!providers.is_hashed());
    assert!(providers.binds(&key("bound@1.0.0")));
    assert!(!providers.binds(&key("own@2.0.0")));
}

#[test]
fn bound_providers_of_snapshot_recovers_hashed_peers_across_shared_descendant_branches() {
    let suffix = PeerBindings::Explicit(vec!["p@1.0.0".to_string(), "q@1.0.0".to_string()])
        .to_suffix(3)
        .expect("hashed suffix");
    let root_key = key(&format!("root@1.0.0{suffix}"));
    let yaml = format!(
        r"lockfileVersion: '9.0'
packages:
  root@1.0.0:
    resolution: {{integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==}}
  branch-p@1.0.0:
    resolution: {{integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==}}
  branch-q@1.0.0:
    resolution: {{integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==}}
  shared@1.0.0:
    resolution: {{integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==}}
    peerDependencies:
      p: ^1.0.0
      q: ^1.0.0
snapshots:
  '{root_key}':
    dependencies:
      branch-p: 1.0.0
      branch-q: 1.0.0
    transitivePeerDependencies:
      - p
      - q
  branch-p@1.0.0:
    dependencies:
      shared: 1.0.0
    transitivePeerDependencies:
      - p
  branch-q@1.0.0:
    dependencies:
      shared: 1.0.0
    transitivePeerDependencies:
      - q
  shared@1.0.0:
    dependencies:
      p: 1.0.0
      q: 1.0.0
",
    );
    let lockfile: Lockfile = serde_saphyr::from_str(&yaml).expect("parse lockfile");
    let providers = BoundProviders::of_snapshot(
        crate::LockfileEntries {
            packages: lockfile.packages.as_ref(),
            snapshots: lockfile.snapshots.as_ref(),
        },
        &root_key,
    );
    assert!(!providers.is_hashed());
    assert!(providers.binds(&key("p@1.0.0")));
    assert!(providers.binds(&key("q@1.0.0")));
}

#[test]
fn bound_providers_of_snapshot_counts_distinct_snapshots_across_multiple_peers() {
    let suffix = PeerBindings::Explicit(vec!["p@1.0.0".to_string(), "q@1.0.0".to_string()])
        .to_suffix(3)
        .expect("hashed suffix");
    let root_key = key(&format!("root@1.0.0{suffix}"));
    let mut snapshots = std::collections::HashMap::new();
    let mut root_deps = std::collections::HashMap::new();
    for idx in 0..600 {
        let name = format!("node-{idx}");
        root_deps.insert(
            name.parse().expect("pkg name"),
            crate::SnapshotDepRef::Plain("1.0.0".parse().expect("ver")),
        );
        let mut deps = std::collections::HashMap::new();
        deps.insert(
            "p".parse().expect("pkg name"),
            crate::SnapshotDepRef::Plain("1.0.0".parse().expect("ver")),
        );
        deps.insert(
            "q".parse().expect("pkg name"),
            crate::SnapshotDepRef::Plain("1.0.0".parse().expect("ver")),
        );
        snapshots.insert(
            key(&format!("{name}@1.0.0")),
            crate::SnapshotEntry { dependencies: Some(deps), ..crate::SnapshotEntry::default() },
        );
    }
    snapshots.insert(
        root_key.clone(),
        crate::SnapshotEntry {
            dependencies: Some(root_deps),
            transitive_peer_dependencies: Some(vec!["p".to_string(), "q".to_string()]),
            ..crate::SnapshotEntry::default()
        },
    );
    let yaml = r"lockfileVersion: '9.0'
packages:
  root@1.0.0:
    resolution: {integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==}
";
    let lockfile: Lockfile = serde_saphyr::from_str(yaml).expect("parse lockfile");
    let providers = BoundProviders::of_snapshot(
        crate::LockfileEntries {
            packages: lockfile.packages.as_ref(),
            snapshots: Some(&snapshots),
        },
        &root_key,
    );
    assert!(!providers.is_hashed());
    assert!(providers.binds(&key("p@1.0.0")));
    assert!(providers.binds(&key("q@1.0.0")));
}

#[test]
fn an_explicit_suffix_names_its_providers_in_order() {
    let bindings = PeerBindings::from_key(&key("a@1.0.0(b@2.0.0(c@1.0.0))(c@1.0.0)")).unwrap();
    let providers: Vec<String> = bindings
        .providers()
        .map(|key| key.to_string())
        .collect();
    assert_eq!(providers, ["b@2.0.0(c@1.0.0)", "c@1.0.0"]);
}

#[test]
fn a_patch_hash_segment_names_no_provider() {
    let bindings = PeerBindings::from_key(&key("a@1.0.0(patch_hash=abc)(b@2.0.0)")).unwrap();
    let providers: Vec<String> = bindings
        .providers()
        .map(|key| key.to_string())
        .collect();
    assert_eq!(providers, ["b@2.0.0"]);
}

#[test]
fn a_hashed_suffix_names_no_provider() {
    let bindings =
        PeerBindings::from_key(&key("a@1.0.0(0123456789abcdef0123456789abcdef)")).unwrap();
    assert_eq!(bindings, PeerBindings::Hashed);
    assert_eq!(bindings.providers().count(), 0);
    assert_eq!(bindings.to_suffix(1000), None);
}

#[test]
fn a_key_without_peers_has_no_bindings() {
    assert_eq!(PeerBindings::from_key(&key("a@1.0.0")), None);
}

#[test]
fn an_over_long_suffix_is_written_as_its_hash() {
    let bindings = PeerBindings::Explicit(vec!["b@2.0.0".to_string()]);
    assert_eq!(bindings.to_suffix(1000).as_deref(), Some("(b@2.0.0)"));
    let hashed = bindings.to_suffix(3).unwrap();
    assert_eq!(PeerBindings::from_suffix(&hashed), Some(PeerBindings::Hashed));
}

#[test]
fn a_patch_marker_stays_in_front_of_the_sorted_providers() {
    let suffix = "(patch_hash=abc)(a@1.0.0)(z@1.0.0)";
    let bindings = PeerBindings::from_suffix(suffix).unwrap();
    assert_eq!(bindings.to_suffix(1000).as_deref(), Some(suffix));
    let hashed = bindings.to_suffix(3).unwrap();
    assert!(hashed.starts_with("(patch_hash=abc)("), "{hashed}");
    assert_eq!(PeerBindings::from_suffix(&hashed), Some(PeerBindings::Hashed));
}

#[test]
fn a_hash_looking_last_segment_after_providers_is_no_hash() {
    let suffix = "(a@1.0.0)(0123456789abcdef0123456789abcdef)";
    assert!(matches!(PeerBindings::from_suffix(suffix), Some(PeerBindings::Explicit(_))));
}

/// Every lockfile checked into the repository, outside `node_modules`.
fn checked_in_lockfiles(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if !matches!(name.to_str(), Some("node_modules" | "target" | ".git" | ".pnpm")) {
                checked_in_lockfiles(&path, found);
            }
        } else if name == "pnpm-lock.yaml" {
            found.push(path);
        }
    }
}

/// Reading a key's bindings and writing them back gives the key's suffix,
/// for every snapshot of every lockfile in the repository. A lockfile
/// this version of pnpm cannot load is skipped.
#[test]
fn bindings_round_trip_through_every_checked_in_lockfile() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let mut lockfiles = Vec::new();
    checked_in_lockfiles(&repo_root, &mut lockfiles);
    let mut checked = 0;
    for path in &lockfiles {
        let Ok(Some(lockfile)) = Lockfile::load_from_path(path) else { continue };
        for snapshot_key in lockfile.snapshots
            .iter()
            .flatten()
            .map(|(key, _)| key)
        {
            let Some(bindings) = PeerBindings::from_key(snapshot_key) else { continue };
            if bindings == PeerBindings::Hashed {
                continue;
            }
            assert_eq!(
                bindings.to_suffix(usize::MAX).as_deref(),
                Some(snapshot_key.suffix.peer()),
                "{snapshot_key} in {}",
                path.display(),
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "no snapshot with peers found under {}", repo_root.display());
}
