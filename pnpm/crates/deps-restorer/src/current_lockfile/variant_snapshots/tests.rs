use super::with_selected_variant_snapshots;
use pnpm_lockfile::{HostFloors, Lockfile, PlatformSelector};
use pretty_assertions::assert_eq;

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
        - resolution: {integrity: sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==}
          targets:
            - os: linux
              cpu: x64
      fallback:
        integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==
  semver@7.7.0:
    resolution: {integrity: sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC==}
  libvips@1.0.0:
    resolution: {integrity: sha512-DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD==}
snapshots:
  sharp@0.35.0:
    dependencies:
      semver: 7.7.0
  sharp@0.35.0/variants/linux-x64:
    dependencies:
      libvips: 1.0.0
      semver: 7.7.0
  semver@7.7.0: {}
  libvips@1.0.0: {}
";

fn host(os: &str) -> PlatformSelector {
    PlatformSelector {
        os: os.to_string(),
        cpu: "x64".to_string(),
        libc: None,
        floors: HostFloors::default(),
    }
}

fn dependency_names(lockfile: &Lockfile) -> Vec<String> {
    let snapshots = lockfile.snapshots.as_ref().unwrap();
    assert!(
        snapshots
            .keys()
            .all(|key| !key.to_string().contains("/variants/")),
        "the variants' snapshot keys are gone",
    );
    let sharp = &snapshots[&"sharp@0.35.0".parse().unwrap()];
    let mut names: Vec<String> = sharp.dependencies
        .iter()
        .flatten()
        .map(|(name, _)| name.to_string())
        .collect();
    names.sort();
    names
}

#[test]
fn the_selected_variant_snapshot_replaces_the_versions() {
    let lockfile =
        Lockfile::parse(LOCKFILE, std::path::Path::new("pnpm-lock.yaml")).unwrap().unwrap();
    let linux = with_selected_variant_snapshots(&lockfile, &host("linux")).unwrap();
    assert_eq!(dependency_names(&linux), ["libvips", "semver"]);
    let darwin = with_selected_variant_snapshots(&lockfile, &host("darwin")).unwrap();
    assert_eq!(dependency_names(&darwin), ["semver"]);
}
