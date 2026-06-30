use crate::{
    DirectoryResolution, ImporterDepVersion, Lockfile, LockfileResolution, PackageKey, PkgName,
    SnapshotDepRef,
};
use pretty_assertions::assert_eq;
use tempfile::tempdir;
use text_block_macros::text_block;

/// Single-document lockfile body shared across the loader tests below.
const MAIN_DOC: &str = text_block! {
    "lockfileVersion: '9.0'"
    ""
    "settings:"
    "  autoInstallPeers: true"
    "  excludeLinksFromLockfile: false"
    ""
    "importers:"
    ""
    "  .:"
    "    dependencies:"
    "      react:"
    "        specifier: ^17.0.2"
    "        version: 17.0.2"
    ""
    "packages:"
    ""
    "  react@17.0.2:"
    "    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}"
    ""
    "snapshots:"
    ""
    "  react@17.0.2: {}"
};

/// Env-document prelude pnpm v11 writes when `packageManager` /
/// `devEngines.runtime` triggers a package-manager-bootstrap entry.
/// Shape matches upstream's `EnvLockfile` at
/// <https://github.com/pnpm/pnpm/blob/31858c544b/lockfile/types/src/index.ts#L187-L194>.
const ENV_DOC: &str = text_block! {
    "lockfileVersion: '9.0'"
    ""
    "importers:"
    ""
    "  .:"
    "    configDependencies: {}"
    "    packageManagerDependencies:"
    "      pnpm:"
    "        specifier: ^11.0.0"
    "        version: 11.0.8"
    ""
    "packages:"
    ""
    "  pnpm@11.0.8:"
    "    resolution: {integrity: sha512-TECX4d0tQjcsTn+lp5H/KPx1pITHrBkuZLHfD97xdZS6mC+bT+2a37PHV4RvVlt5mydj+zcz0d4by4LPRmhJEg==}"
    "    hasBin: true"
    ""
    "snapshots:"
    ""
    "  pnpm@11.0.8: {}"
};

fn write_lockfile(content: &str) -> tempfile::TempDir {
    let tmp = tempdir().expect("create tempdir");
    let virtual_store_dir = tmp.path().join("node_modules").join(".pacquet");
    std::fs::create_dir_all(&virtual_store_dir).expect("mkdir virtual_store_dir");
    std::fs::write(virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME), content)
        .expect("write lock.yaml");
    tmp
}

#[test]
fn parses_main_document_from_combined_yaml() {
    let combined = format!("---\n{ENV_DOC}\n---\n{MAIN_DOC}");
    let tmp = write_lockfile(&combined);
    let virtual_store_dir = tmp.path().join("node_modules").join(".pacquet");

    let combined_loaded = Lockfile::load_current_from_virtual_store_dir(&virtual_store_dir)
        .expect("load combined lockfile")
        .expect("combined lockfile should be present");

    let tmp_main = write_lockfile(MAIN_DOC);
    let main_only_dir = tmp_main.path().join("node_modules").join(".pacquet");
    let main_only_loaded = Lockfile::load_current_from_virtual_store_dir(&main_only_dir)
        .expect("load main-only lockfile")
        .expect("main-only lockfile should be present");

    assert_eq!(combined_loaded, main_only_loaded);
}

#[test]
fn env_only_lockfile_loads_as_none() {
    let env_only = format!("---\n{ENV_DOC}\n");
    let tmp = write_lockfile(&env_only);
    let virtual_store_dir = tmp.path().join("node_modules").join(".pacquet");

    let result = Lockfile::load_current_from_virtual_store_dir(&virtual_store_dir)
        .expect("env-only lockfile should not error");
    assert!(result.is_none(), "expected None for env-only lockfile, got: {result:?}");
}

/// Parity port of the TS heuristic-boundary test
/// `lockfile/fs/test/lockfileV6Converters.test.ts::convertToLockfileObject()
/// reconstructs a dropped directory resolution for a pruned file:
/// peer-variant, but never for a file: tarball`.
#[test]
fn reconstructs_dropped_directory_resolution_for_pruned_file_peer_variant() {
    let pruned = text_block! {
        "lockfileVersion: '9.0'"
        ""
        "importers: {}"
        ""
        "snapshots:"
        ""
        "  dir@file:packages/dir(peer@1.0.0): {}"
        "  tar@file:vendor/tar-1.0.0.tgz(peer@1.0.0): {}"
        "  upper@file:vendor/upper-1.0.0.TGZ(peer@1.0.0): {}"
        "  mixed@file:vendor/mixed-1.0.0.Tar.Gz(peer@1.0.0): {}"
    };
    let tmp = write_lockfile(pruned);
    let virtual_store_dir = tmp.path().join("node_modules").join(".pacquet");
    let lockfile = Lockfile::load_current_from_virtual_store_dir(&virtual_store_dir)
        .expect("load pruned lockfile")
        .expect("pruned lockfile should be present");

    let packages = lockfile.packages.as_ref().expect("packages synthesized for dir entry");

    let dir_key: PackageKey = "dir@file:packages/dir".parse().expect("parse dir key");
    let dir_metadata = packages.get(&dir_key).expect("dir entry synthesized");
    assert_eq!(
        dir_metadata.resolution,
        LockfileResolution::Directory(DirectoryResolution {
            directory: "packages/dir".to_string()
        }),
    );

    for tarball_key in [
        "tar@file:vendor/tar-1.0.0.tgz",
        "upper@file:vendor/upper-1.0.0.TGZ",
        "mixed@file:vendor/mixed-1.0.0.Tar.Gz",
    ] {
        let key: PackageKey = tarball_key.parse().expect("parse tarball key");
        assert!(
            packages.get(&key).is_none(),
            "tarball `{tarball_key}` must not get a synthesized directory resolution",
        );
    }
}

/// Regression for [pnpm/pnpm#11776](https://github.com/pnpm/pnpm/issues/11776):
/// a lockfile whose importer dependency version is a GitHub codeload
/// tarball URL used to crash the loader with `Failed to parse the
/// version part: Failed to parse version`. The URL is the upstream
/// `nonSemverVersion` shape and must round-trip through the loader as
/// an `ImporterDepVersion::Regular` with a non-semver version slot,
/// plus parse as a `packages:` / `snapshots:` key under the same URL.
#[test]
fn loads_importer_dep_with_codeload_tarball_url_version() {
    let url = "https://codeload.github.com/whiskeysockets/libsignal-node/tar.gz/0848bc83347720c322c5087f3bd0d6cd086ffa4b";
    let yaml = format!(
        "\
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      libsignal:
        specifier: {url}
        version: {url}

packages:

  libsignal@{url}:
    resolution: {{tarball: {url}}}
    version: 2.0.1

snapshots:

  libsignal@{url}: {{}}
",
    );
    let tmp = write_lockfile(&yaml);
    let virtual_store_dir = tmp.path().join("node_modules").join(".pacquet");
    let lockfile = Lockfile::load_current_from_virtual_store_dir(&virtual_store_dir)
        .expect("load codeload-url lockfile")
        .expect("codeload-url lockfile should be present");

    let importer = lockfile.root_project().expect("root importer present");
    let deps = importer.dependencies.as_ref().expect("importer has dependencies");
    let libsignal_name: PkgName = "libsignal".parse().expect("parse libsignal name");
    let spec = deps.get(&libsignal_name).expect("libsignal dep present");
    assert_eq!(spec.specifier, url);
    let regular = match &spec.version {
        ImporterDepVersion::Regular(ver_peer) => ver_peer,
        other => panic!("expected Regular, got {other:?}"),
    };
    assert_eq!(regular.to_string(), url);

    let key: PackageKey = format!("libsignal@{url}").parse().expect("parse package key");
    let packages = lockfile.packages.as_ref().expect("packages present");
    assert!(packages.contains_key(&key));
    let snapshots = lockfile.snapshots.as_ref().expect("snapshots present");
    assert!(snapshots.contains_key(&key));
}

/// Regression test for <https://github.com/pnpm/pnpm/issues/11775>.
/// An injected workspace package's snapshot can hold a `link:<path>`
/// value in its `dependencies:` map when the dep is a workspace
/// sibling. Pnpm's own parser accepts the shape — `refToRelative`
/// short-circuits to `null` for `link:` references at use time — so
/// pacquet must too.
#[test]
fn parses_link_dep_in_injected_snapshot() {
    let lockfile_text = text_block! {
        "lockfileVersion: '9.0'"
        ""
        "settings:"
        "  autoInstallPeers: true"
        "  excludeLinksFromLockfile: false"
        ""
        "importers:"
        ""
        "  .: {}"
        ""
        "  packages/a:"
        "    dependencies:"
        "      b:"
        "        specifier: workspace:^"
        "        version: file:packages/b"
        "    dependenciesMeta:"
        "      b:"
        "        injected: true"
        ""
        "  packages/b:"
        "    dependencies:"
        "      c:"
        "        specifier: workspace:^"
        "        version: link:../c"
        ""
        "  packages/c: {}"
        ""
        "packages:"
        ""
        "  b@file:packages/b:"
        "    resolution: {directory: packages/b, type: directory}"
        ""
        "snapshots:"
        ""
        "  b@file:packages/b:"
        "    dependencies:"
        "      c: link:packages/c"
    };
    let tmp = write_lockfile(lockfile_text);
    let virtual_store_dir = tmp.path().join("node_modules").join(".pacquet");

    let lockfile = Lockfile::load_current_from_virtual_store_dir(&virtual_store_dir)
        .expect("load lockfile with link: snapshot dep")
        .expect("lockfile should be present");

    let snapshots = lockfile.snapshots.as_ref().expect("snapshots present");
    let b_key: PackageKey = "b@file:packages/b".parse().expect("parse b key");
    let b_snapshot = snapshots.get(&b_key).expect("b snapshot present");
    let deps = b_snapshot.dependencies.as_ref().expect("b deps present");

    let c_name = PkgName::parse("c").expect("parse c");
    let c_ref = deps.get(&c_name).expect("c entry present");
    assert_eq!(c_ref, &SnapshotDepRef::Link("packages/c".to_string()));
    assert_eq!(c_ref.resolve(&c_name), None);
}
