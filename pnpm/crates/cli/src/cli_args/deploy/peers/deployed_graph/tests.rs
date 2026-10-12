use super::{DeployedGraph, ancestor_peer_providers, dependents_by_snapshot};
use pnpm_lockfile::{
    PkgName, PkgNameVerPeer, ProjectSnapshot, ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
};
use pretty_assertions::assert_eq;
use std::collections::HashMap;

fn key(text: &str) -> PkgNameVerPeer {
    text.parse().unwrap()
}

fn reference(text: &str) -> SnapshotDepRef {
    text.parse().unwrap()
}

/// A snapshot depending on `dependencies`, each an `(alias, reference)` pair.
fn snapshot(dependencies: &[(&str, &str)]) -> SnapshotEntry {
    SnapshotEntry {
        dependencies: Some(
            dependencies
                .iter()
                .map(|(alias, target)| (alias.parse().unwrap(), reference(target)))
                .collect(),
        ),
        ..SnapshotEntry::default()
    }
}

/// The deployed graph [`providers_in`] searches.
struct Fixture<'a> {
    /// The deployed project's dependencies, each an `(alias, version)` pair.
    importer: &'a [(&'a str, &'a str)],
    /// Snapshots besides `lib@1.0.0`, each with its `(alias, reference)` pairs.
    snapshots: &'a [(&'a str, &'a [(&'a str, &'a str)])],
    /// The `(snapshot, alias)` edges that satisfy a peer of the snapshot.
    peer_edges: &'a [(&'a str, &'a str)],
}

/// The providers of `peer` for `lib@1.0.0` in `fixture`.
fn providers_in(fixture: &Fixture<'_>) -> Vec<SnapshotDepRef> {
    let mut snapshots = HashMap::from([(key("lib@1.0.0"), SnapshotEntry::default())]);
    for (name, dependencies) in fixture.snapshots {
        snapshots.insert(key(name), snapshot(dependencies));
    }
    let importer = ProjectSnapshot {
        dependencies: Some(
            fixture.importer
                .iter()
                .map(|(alias, version)| {
                    let spec = ResolvedDependencySpec {
                        specifier: (*version).to_string(),
                        version: version.parse().unwrap(),
                    };
                    (alias.parse().unwrap(), spec)
                })
                .collect(),
        ),
        ..ProjectSnapshot::default()
    };
    let graph = DeployedGraph {
        importer: Some(&importer),
        snapshots: &snapshots,
        candidates: HashMap::default(),
        dependents: dependents_by_snapshot(Some(&importer), &snapshots, |parent, alias| {
            fixture.peer_edges
                .iter()
                .any(|(peer_parent, peer_alias)| {
                    key(peer_parent) == *parent && alias.to_string() == *peer_alias
                })
        }),
    };
    let peer: PkgName = "peer".parse().unwrap();
    ancestor_peer_providers(&graph, &key("lib@1.0.0"), &peer)
}

/// The providers of `peer` for `lib@1.0.0`, which `mid@1.0.0` and
/// `mid-2@1.0.0` depend on.
fn providers(mid: &[(&str, &str)], mid_2: &[(&str, &str)]) -> Vec<SnapshotDepRef> {
    providers_in(&Fixture {
        importer: &[],
        snapshots: &[("mid@1.0.0", mid), ("mid-2@1.0.0", mid_2)],
        peer_edges: &[],
    })
}

#[test]
fn two_spellings_of_one_package_count_once() {
    let found = providers(
        &[("lib", "1.0.0"), ("peer", "1.0.0")],
        &[("lib", "1.0.0"), ("peer", "peer@1.0.0")],
    );
    assert_eq!(found.len(), 1);
}

#[test]
fn two_packages_at_one_version_are_distinct() {
    let found = providers(
        &[("lib", "1.0.0"), ("peer", "1.0.0")],
        &[("lib", "1.0.0"), ("peer", "other@1.0.0")],
    );
    assert_eq!(found.len(), 2);
}

/// A `<root>/` link points into the package that declares it, so it names a
/// different package than any other provider.
#[test]
fn a_link_into_the_providing_package_is_its_own_provider() {
    let found = providers(
        &[("lib", "1.0.0"), ("peer", "link:<root>/vendor/peer")],
        &[("lib", "1.0.0"), ("peer", "1.0.0")],
    );
    assert_eq!(found.len(), 2);
}

/// The deployed project and a snapshot each link `peer` into their own
/// package, so the two links name different packages however they are spelled.
#[test]
fn a_link_into_the_deployed_project_is_its_own_provider() {
    let found = providers_in(&Fixture {
        importer: &[("lib", "1.0.0"), ("peer", "link:<root>/vendor/peer")],
        snapshots: &[("mid@1.0.0", &[("lib", "1.0.0"), ("peer", "link:<root>/vendor/peer")])],
        peer_edges: &[],
    });
    assert_eq!(found.len(), 2);
}

/// `sibling` consumes `lib` as a peer, so it is not `lib`'s parent, and its own
/// `peer` does not compete with the deployed project's.
#[test]
fn a_package_consuming_the_linked_package_as_a_peer_is_not_its_parent() {
    let found = providers_in(&Fixture {
        importer: &[("lib", "1.0.0"), ("sibling", "1.0.0"), ("peer", "1.0.0")],
        snapshots: &[("sibling@1.0.0", &[("lib", "1.0.0"), ("peer", "2.0.0")])],
        peer_edges: &[("sibling@1.0.0", "lib")],
    });
    assert_eq!(found, vec![reference("1.0.0")]);
}
