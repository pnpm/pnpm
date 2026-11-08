use super::{PlatformArchivesInput, StoredArchive, platform_archives};
use pnpm_lockfile::{
    HostFloors, Lockfile, PackageKey, PeerSatisfactionEdges, PlatformSelector, SnapshotEntry,
};
use pretty_assertions::assert_eq;
use std::collections::HashMap;

/// `sharp` has an `any` archive, a darwin variant whose own snapshot depends
/// on `darwin-vips`, and a linux variant whose own snapshot depends on
/// `linux-vips`. Both versions' own snapshot depends on `semver`.
const LOCKFILE: &str = "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      sharp:
        specifier: 0.35.0
        version: 0.35.0
packages:
  sharp@0.35.0:
    resolution:
      type: variations
      variants:
        - resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==}
          targets:
            - any: true
        - resolution: {integrity: sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==}
          targets:
            - os: darwin
              cpu: arm64
        - resolution: {integrity: sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC==}
          targets:
            - os: linux
              cpu: x64
  semver@7.7.0:
    resolution: {integrity: sha512-DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD==}
  darwin-vips@1.0.0:
    resolution: {integrity: sha512-EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE==}
  linux-vips@1.0.0:
    resolution: {integrity: sha512-FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF==}
snapshots:
  sharp@0.35.0:
    dependencies:
      semver: 7.7.0
  sharp@0.35.0/variants/darwin-arm64:
    dependencies:
      darwin-vips: 1.0.0
      semver: 7.7.0
  sharp@0.35.0/variants/linux-x64:
    dependencies:
      linux-vips: 1.0.0
      semver: 7.7.0
  semver@7.7.0: {}
  darwin-vips@1.0.0: {}
  linux-vips@1.0.0: {}
";

fn platform(os: &str, cpu: &str) -> PlatformSelector {
    PlatformSelector {
        os: os.to_string(),
        cpu: cpu.to_string(),
        libc: None,
        floors: Some(HostFloors::default()),
    }
}

fn key(text: &str) -> PackageKey {
    text.parse().unwrap()
}

/// What the store keeps for `platforms` beside an install that links the
/// darwin variant of `sharp` and its dependencies.
fn kept_on_darwin(platforms: &[PlatformSelector]) -> Vec<(PackageKey, StoredArchive)> {
    let lockfile =
        Lockfile::parse(LOCKFILE, std::path::Path::new("pnpm-lock.yaml")).unwrap().unwrap();
    let materialized: HashMap<PackageKey, SnapshotEntry> =
        ["sharp@0.35.0", "semver@7.7.0", "darwin-vips@1.0.0"]
            .into_iter()
            .map(|text| (key(text), SnapshotEntry::default()))
            .collect();
    let linked = platform("darwin", "arm64");
    let input = PlatformArchivesInput {
        lockfile: &lockfile,
        roots: &lockfile.importers,
        materialized: &materialized,
        linked: &linked,
        skipped_peer_edges: &PeerSatisfactionEdges::default(),
        include_optional: true,
    };
    platform_archives(&input, platforms)
        .into_iter()
        .map(|kept| (kept.key, kept.archive))
        .collect()
}

#[test]
fn another_platforms_variant_is_kept_with_the_dependencies_of_its_own_snapshot() {
    assert_eq!(
        kept_on_darwin(&[platform("linux", "x64")]),
        [
            (key("sharp@0.35.0"), StoredArchive::Variant(2)),
            (key("linux-vips@1.0.0"), StoredArchive::Own)
        ],
    );
}

#[test]
fn the_linked_platform_keeps_nothing_the_install_links() {
    assert_eq!(kept_on_darwin(&[platform("darwin", "arm64")]), []);
}

/// A platform no variant serves keeps the archive that targets `any`, and
/// the dependencies of the version's own snapshot, which the install already
/// links. Neither variant's own dependency is kept.
#[test]
fn a_platform_no_variant_serves_keeps_the_any_archive_and_no_variants_dependency() {
    assert_eq!(
        kept_on_darwin(&[platform("win32", "x64")]),
        [(key("sharp@0.35.0"), StoredArchive::Variant(0))],
    );
}

mod edges;
mod stored;
