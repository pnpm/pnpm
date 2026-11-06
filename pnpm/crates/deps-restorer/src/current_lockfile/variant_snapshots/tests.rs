use super::{has_variant_snapshots, split_variant_key, with_selected_variant_snapshots};
use pnpm_lockfile::{Lockfile, PlatformSelector};
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
        - resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==}
          targets:
            - any: true
        - resolution: {integrity: sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==}
          targets:
            - os: linux
              cpu: x64
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
    PlatformSelector { os: os.to_string(), cpu: "x64".to_string(), libc: None, floors: None }
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

#[test]
fn a_file_dependency_in_a_directory_named_variants_is_no_variant_snapshot() {
    let lockfile = "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      local:
        specifier: file:../variants/local
        version: file:../variants/local
packages:
  local@file:../variants/local:
    resolution: {directory: ../variants/local, type: directory}
snapshots:
  local@file:../variants/local: {}
";
    let lockfile =
        Lockfile::parse(lockfile, std::path::Path::new("pnpm-lock.yaml")).unwrap().unwrap();
    assert!(!has_variant_snapshots(&lockfile));
    assert!(with_selected_variant_snapshots(&lockfile, &host("linux")).is_none());
}

#[test]
fn variant_keys_are_split_after_the_name_and_a_named_registry() {
    let split = |key: &str| {
        split_variant_key(&key.parse().unwrap()).map(|(base, tag)| (base.to_string(), tag))
    };
    assert_eq!(
        split("@img/sharp@0.35.0/variants/linux-x64(react@18.3.1)"),
        Some(("@img/sharp@0.35.0(react@18.3.1)".to_string(), "linux-x64".to_string())),
    );
    assert_eq!(
        split("sharp@npmjs:0.35.0/variants/linux-x64"),
        Some(("sharp@npmjs:0.35.0".to_string(), "linux-x64".to_string())),
    );
    assert_eq!(split("local@file:../variants/local"), None);
    assert_eq!(split("local@file:1.2.3/variants/local"), None);
    assert_eq!(split("foo@1.0.0(file:../variants/local)(z@1.0.0)"), None);
}

/// A filtered install writes the whole wanted lockfile, which is the same on
/// every platform: it keeps each variant's snapshot and the packages only a
/// variant depends on, whichever variant this host links.
#[test]
fn a_filtered_install_keeps_every_variant_in_the_wanted_lockfile() {
    let lockfile =
        Lockfile::parse(LOCKFILE, std::path::Path::new("pnpm-lock.yaml")).unwrap().unwrap();
    let importers = std::collections::HashSet::from([".".to_string()]);

    let merged = crate::current_lockfile::merge_filtered_wanted_lockfile(
        None,
        lockfile.clone(),
        &importers,
        &importers,
        std::path::Path::new("/workspace"),
    )
    .expect("every importer is resolved");

    assert_eq!(merged.snapshots, lockfile.snapshots);
}

const CONFLICTING: &str = "lockfileVersion: '9.0'
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
            - os: linux
              cpu: x64
        - resolution: {integrity: sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC==}
          targets:
            - os: darwin
              cpu: arm64
        - resolution: {integrity: sha512-DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD==}
          targets:
            - os: win32
              cpu: x64
  semver@7.7.0:
    resolution: {integrity: sha512-EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE==}
  semver@7.6.0:
    resolution: {integrity: sha512-FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF==}
  libvips@1.0.0:
    resolution: {integrity: sha512-GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG==}
  libvips@2.0.0:
    resolution: {integrity: sha512-HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH==}
  lru-cache@6.0.0:
    resolution: {integrity: sha512-IIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII==}
  glib@2.0.0:
    resolution: {integrity: sha512-JJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJJ==}
snapshots:
  sharp@0.35.0:
    dependencies:
      semver: 7.7.0
  sharp@0.35.0/variants/linux-x64:
    dependencies:
      semver: 7.6.0
  sharp@0.35.0/variants/darwin-arm64:
    dependencies:
      libvips: 1.0.0
  sharp@0.35.0/variants/win32-x64:
    dependencies:
      libvips: 2.0.0
  semver@7.7.0: {}
  semver@7.6.0:
    dependencies:
      lru-cache: 6.0.0
  lru-cache@6.0.0: {}
  libvips@1.0.0: {}
  libvips@2.0.0:
    dependencies:
      glib: 2.0.0
  glib@2.0.0: {}
";

/// A version's own snapshot and its variants, or two of its variants, can
/// resolve one alias to different versions: the wanted lockfile keeps each
/// version and what it depends on.
#[test]
fn a_filtered_install_keeps_each_version_its_variants_resolve_an_alias_to() {
    let lockfile =
        Lockfile::parse(CONFLICTING, std::path::Path::new("pnpm-lock.yaml")).unwrap().unwrap();
    let importers = std::collections::HashSet::from([".".to_string()]);

    let merged = crate::current_lockfile::merge_filtered_wanted_lockfile(
        None,
        lockfile.clone(),
        &importers,
        &importers,
        std::path::Path::new("/workspace"),
    )
    .expect("every importer is resolved");

    assert_eq!(merged.snapshots, lockfile.snapshots);
    assert_eq!(merged.packages, lockfile.packages);
}
