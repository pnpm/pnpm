use super::super::{KeptArchive, PlatformArchivesInput, StoredArchive, platform_archives};
use pnpm_lockfile::{
    HostFloors, Lockfile, PackageKey, PeerEdgeOptions, PeerSatisfactionEdges, PlatformSelector,
    SnapshotEntry,
};
use pretty_assertions::assert_eq;
use std::collections::HashMap;

fn linux() -> PlatformSelector {
    PlatformSelector {
        os: "linux".to_string(),
        cpu: "x64".to_string(),
        libc: None,
        floors: Some(HostFloors::default()),
    }
}

fn darwin() -> PlatformSelector {
    PlatformSelector { os: "darwin".to_string(), cpu: "arm64".to_string(), ..linux() }
}

fn parse(yaml: &str) -> Lockfile {
    Lockfile::parse(yaml, std::path::Path::new("pnpm-lock.yaml")).unwrap().unwrap()
}

/// What `lockfile` keeps for `linux` beside an install on darwin that
/// materializes `materialized`, with `skipped_peer_edges` left out of the
/// walk and the dev dependencies of every project dropped, as
/// `pnpm install --prod` drops them.
fn kept_for_production(
    lockfile: &Lockfile,
    materialized: &[&str],
    skipped_peer_edges: &PeerSatisfactionEdges,
) -> Vec<KeptArchive> {
    let mut roots = lockfile.importers.clone();
    for importer in roots.values_mut() {
        importer.dev_dependencies = None;
    }
    let materialized: HashMap<PackageKey, SnapshotEntry> = materialized
        .iter()
        .map(|key| (key.parse().unwrap(), SnapshotEntry::default()))
        .collect();
    let linked = darwin();
    let input = PlatformArchivesInput {
        lockfile,
        roots: &roots,
        materialized: &materialized,
        linked: &linked,
        skipped_peer_edges,
        include_optional: true,
    };
    platform_archives(&input, &[linux()])
}

fn own(key: &str, optional: bool) -> KeptArchive {
    KeptArchive { key: key.parse().unwrap(), archive: StoredArchive::Own, optional }
}

/// `host` has `peer-c` as an optional peer, which the project provides as a
/// dev dependency.
const OPTIONAL_PEER: &str = "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      host:
        specifier: 1.0.0
        version: 1.0.0(peer-c@1.0.0)
    devDependencies:
      peer-c:
        specifier: 1.0.0
        version: 1.0.0
packages:
  host@1.0.0:
    resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==}
    peerDependencies:
      peer-c: 1.0.0
    peerDependenciesMeta:
      peer-c:
        optional: true
  peer-c@1.0.0:
    resolution: {integrity: sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==}
snapshots:
  host@1.0.0(peer-c@1.0.0):
    dependencies:
      peer-c: 1.0.0
  peer-c@1.0.0: {}
";

/// The edge that only satisfies a dev-provided optional peer is left out, as
/// the install leaves it out, so a production install keeps no archive for
/// the peer.
#[test]
fn an_edge_that_only_satisfies_an_optional_peer_is_not_followed() {
    let lockfile = parse(OPTIONAL_PEER);
    let materialized = ["host@1.0.0(peer-c@1.0.0)"];
    let edges = PeerSatisfactionEdges::of_lockfile(&lockfile, PeerEdgeOptions::default());

    assert_eq!(kept_for_production(&lockfile, &materialized, &edges), []);
    assert_eq!(
        kept_for_production(&lockfile, &materialized, &PeerSatisfactionEdges::default()),
        [own("peer-c@1.0.0", false)],
        "without the classification the walk would reach the peer",
    );
}

/// Optional binaries for a listed and an unlisted platform: only the listed
/// platform's is kept, as optional, whether its platform is declared or
/// read from its name.
const OPTIONAL_BINARIES: &str = "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      tool:
        specifier: 1.0.0
        version: 1.0.0
packages:
  tool@1.0.0:
    resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==}
  tool-linux@1.0.0:
    resolution: {integrity: sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==}
    os: [linux]
  tool-win32@1.0.0:
    resolution: {integrity: sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC==}
    os: [win32]
  '@tool/linux-x64@1.0.0':
    resolution: {integrity: sha512-DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD==}
  '@tool/freebsd-x64@1.0.0':
    resolution: {integrity: sha512-EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE==}
  git-hosted@1.0.0:
    resolution: {tarball: https://codeload.github.com/acme/git-hosted/tar.gz/0123456789abcdef0123456789abcdef01234567}
snapshots:
  tool@1.0.0:
    dependencies:
      git-hosted: 1.0.0
    optionalDependencies:
      tool-linux: 1.0.0
      tool-win32: 1.0.0
      '@tool/linux-x64': 1.0.0
      '@tool/freebsd-x64': 1.0.0
  tool-linux@1.0.0: {}
  tool-win32@1.0.0: {}
  '@tool/linux-x64@1.0.0': {}
  '@tool/freebsd-x64@1.0.0': {}
  git-hosted@1.0.0: {}
";

#[test]
fn an_optional_dependency_for_another_platform_is_left_out() {
    let lockfile = parse(OPTIONAL_BINARIES);
    let mut kept =
        kept_for_production(&lockfile, &["tool@1.0.0"], &PeerSatisfactionEdges::default());
    kept.sort_by_key(|kept| kept.key.to_string());

    assert_eq!(kept, [own("@tool/linux-x64@1.0.0", true), own("tool-linux@1.0.0", true)]);
}

/// A tarball a git host serves is prepared before it is the package, which
/// may run its scripts, so the store-only walk keeps none.
#[test]
fn a_git_hosted_tarball_is_not_kept() {
    let lockfile = parse(OPTIONAL_BINARIES);
    let kept = kept_for_production(&lockfile, &["tool@1.0.0"], &PeerSatisfactionEdges::default());

    assert!(
        kept.iter()
            .all(|kept| kept.key.to_string() != "git-hosted@1.0.0"),
        "{kept:?}",
    );
}

const REQUIRED_FOREIGN: &str = "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      tool:
        specifier: 1.0.0
        version: 1.0.0
packages:
  tool@1.0.0:
    resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==}
  tool-win32@1.0.0:
    resolution: {integrity: sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC==}
    os: [win32]
  child@1.0.0:
    resolution: {integrity: sha512-DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD==}
snapshots:
  tool@1.0.0:
    dependencies:
      tool-win32: 1.0.0
  tool-win32@1.0.0:
    dependencies:
      child: 1.0.0
  child@1.0.0: {}
";

#[test]
fn a_required_dependency_another_platform_cannot_run_is_kept_with_its_children() {
    let lockfile = parse(REQUIRED_FOREIGN);
    let mut kept =
        kept_for_production(&lockfile, &["tool@1.0.0"], &PeerSatisfactionEdges::default());
    kept.sort_by_key(|kept| kept.key.to_string());

    assert_eq!(kept, [own("child@1.0.0", false), own("tool-win32@1.0.0", false)]);
}
