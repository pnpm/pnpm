use super::{FrozenLocalTarballCheck, frozen_local_tarballs_to_verify};
use pnpm_lockfile::{Lockfile, PlatformSelector};
use std::{collections::HashSet, path::Path};

const LOCKFILE: &str = "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      native:
        specifier: 1.0.0
        version: 1.0.0
packages:
  native@1.0.0:
    resolution:
      type: variations
      variants:
        - resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==}
          targets:
            - any: true
        - resolution: {integrity: sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==}
          targets:
            - os: aix
              cpu: ppc64
  local@file:local.tgz:
    resolution: {integrity: sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC==, tarball: file:local.tgz}
    version: 1.0.0
snapshots:
  native@1.0.0: {}
  native@1.0.0/variants/aix-ppc64:
    dependencies:
      local: file:local.tgz
  local@file:local.tgz: {}
";

/// The local tarballs a frozen install verifies when it links the variant
/// for `linked`, or the host's when `None`.
fn verified(linked: Option<PlatformSelector>) -> usize {
    let lockfile = Lockfile::parse(LOCKFILE, Path::new("pnpm-lock.yaml")).unwrap().unwrap();
    let mut groups = crate::GroupSelection::all();
    groups.linked_platform = linked;
    frozen_local_tarballs_to_verify(&FrozenLocalTarballCheck {
        workspace_root: Path::new("/workspace"),
        importer_ids: &HashSet::from([".".to_string()]),
        groups: &groups,
        lockfile: &lockfile,
        skipped: &pnpm_deps_restorer::SkippedSnapshots::new(),
    })
    .len()
}

#[test]
fn a_local_tarball_the_linked_variant_depends_on_is_verified() {
    let aix = PlatformSelector {
        os: "aix".to_string(),
        cpu: "ppc64".to_string(),
        libc: None,
        floors: None,
    };
    assert_eq!(verified(Some(aix)), 1);
    assert_eq!(verified(None), 0);
}
