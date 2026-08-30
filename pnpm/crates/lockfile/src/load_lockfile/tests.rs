use crate::{
    DirectoryResolution, ImporterDepVersion, LazyLockfile, Lockfile, LockfileResolution,
    PackageKey, PkgName, SnapshotDepRef, WantedLockfileSelection,
};
use pnpm_diagnostics::miette::Diagnostic;
use pretty_assertions::assert_eq;
use std::{collections::BTreeMap, fmt::Write, path::Path};
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
fn fix_loader_discards_broken_and_derived_package_fields() {
    let tmp = tempdir().expect("create tempdir");
    std::fs::write(
        tmp.path().join(Lockfile::FILE_NAME),
        text_block! {
            "lockfileVersion: '9.0'"
            ""
            "settings: invalid"
            ""
            "importers:"
            "  .: {}"
            ""
            "packages:"
            "  broken@1.0.0:"
            "    engines: invalid"
            "  valid@1.0.0:"
            "    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}"
            "    engines: invalid"
            "    deprecated: stale"
            ""
            "snapshots:"
            "  broken@1.0.0: {}"
            "  valid@1.0.0:"
            "    dependencies:"
            "      child: 1.0.0"
            "    transitivePeerDependencies: invalid"
        },
    )
    .expect("write wanted lockfile");

    let lazy = LazyLockfile::deferred(tmp.path().to_path_buf(), WantedLockfileSelection::default());
    let lockfile = lazy.get_for_fix().expect("load for repair").expect("lockfile present");
    assert!(lockfile.settings.is_none());
    let packages = lockfile.packages.as_ref().expect("packages present");
    assert!(!packages.contains_key(&"broken@1.0.0".parse().expect("broken key")));
    let valid = packages.get(&"valid@1.0.0".parse().expect("valid key")).expect("valid entry");
    assert!(valid.engines.is_none());
    assert!(valid.deprecated.is_none());

    let snapshots = lockfile.snapshots.as_ref().expect("snapshots present");
    let valid =
        snapshots.get(&"valid@1.0.0".parse().expect("valid snapshot key")).expect("valid snapshot");
    assert!(valid.dependencies.as_ref().is_some_and(|deps| deps.len() == 1));
    assert!(valid.transitive_peer_dependencies.is_none());
}

/// Regression test for <https://github.com/pnpm/pnpm/issues/13606>: a
/// combined lockfile checked out with CRLF line endings was handed to
/// serde whole, failing as "multiple YAML documents detected" and
/// making every install re-resolve from the registry.
#[test]
fn parses_main_document_from_crlf_combined_yaml() {
    let combined = format!("---\n{ENV_DOC}\n---\n{MAIN_DOC}").replace('\n', "\r\n");
    let tmp = write_lockfile(&combined);
    let virtual_store_dir = tmp.path().join("node_modules").join(".pacquet");

    let crlf_loaded = Lockfile::load_current_from_virtual_store_dir(&virtual_store_dir)
        .expect("load CRLF combined lockfile")
        .expect("CRLF combined lockfile should be present");

    let tmp_main = write_lockfile(MAIN_DOC);
    let main_only_dir = tmp_main.path().join("node_modules").join(".pacquet");
    let main_only_loaded = Lockfile::load_current_from_virtual_store_dir(&main_only_dir)
        .expect("load main-only lockfile")
        .expect("main-only lockfile should be present");

    assert_eq!(crlf_loaded, main_only_loaded);
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

#[test]
fn parses_lockfile_larger_than_default_yaml_node_budget() {
    const IMPORTER_COUNT: usize = 130_000;

    let mut content = String::from("lockfileVersion: '9.0'\n\nimporters:\n");
    for index in 0..IMPORTER_COUNT {
        writeln!(content, "  project-{index}: {{}}").expect("write importer");
    }

    let lockfile = Lockfile::parse(&content, Path::new(Lockfile::FILE_NAME))
        .expect("parse large lockfile")
        .expect("large lockfile should be present");

    assert_eq!(lockfile.importers.len(), IMPORTER_COUNT);
}

#[test]
fn parses_lockfile_larger_than_default_yaml_scalar_byte_budget() {
    // A single huge scalar to push the document past the parser's 64 MiB default scalar budget,
    // avoiding the O(N) allocation overhead of creating millions of individual AST nodes.
    let mut content = String::from("lockfileVersion: '9.0'\n\npnpmfileChecksum: ");
    let huge_string_len = 65 * 1024 * 1024;
    content.reserve(huge_string_len + 100);
    content.push_str(&"a".repeat(huge_string_len));
    content.push_str("\n\nimporters:\n  .: {}\n");

    assert!(content.len() > 64 * 1024 * 1024, "fixture must exceed the default scalar budget");

    let lockfile = Lockfile::parse(&content, Path::new(Lockfile::FILE_NAME))
        .expect("parse large lockfile")
        .expect("large lockfile should be present");

    assert!(lockfile.pnpmfile_checksum.is_some());
    assert_eq!(lockfile.pnpmfile_checksum.unwrap().len(), huge_string_len);
}

// A regression here makes every subsequent install re-resolve from
// scratch after failing to read the lockfile it just wrote.
#[test]
fn snapshot_key_over_simple_key_limit_round_trips() {
    let long_key = (0..40).fold(String::from("@scope/pkg@1.0.0"), |mut key, index| {
        write!(key, "(@scope/very-long-peer-dependency-name-{index:02}@33.44.55)")
            .expect("write peer suffix");
        key
    });
    assert!(long_key.len() > 1024, "fixture key must exceed the simple-key limit");

    let content = format!(
        "lockfileVersion: '9.0'\n\nimporters:\n\n  .: {{}}\n\nsnapshots:\n\n  ? '{long_key}'\n  : {{}}\n",
    );
    let lockfile = Lockfile::parse(&content, Path::new(Lockfile::FILE_NAME))
        .expect("parse lockfile with explicit long key")
        .expect("lockfile should be present");
    let key: PackageKey = long_key.parse().expect("parse long snapshot key");
    assert!(lockfile.snapshots.as_ref().expect("snapshots").contains_key(&key));

    let emitted = lockfile.to_yaml_string().expect("emit lockfile");
    assert!(emitted.contains("? '@scope/pkg@1.0.0"), "long key must be emitted in explicit form");
    let reparsed = Lockfile::parse(&emitted, Path::new(Lockfile::FILE_NAME))
        .expect("reparse emitted lockfile")
        .expect("reparsed lockfile should be present");
    assert_eq!(reparsed, lockfile);
}

#[test]
fn parse_error_does_not_include_lockfile_content() {
    let dir = tempdir().expect("create tempdir");
    let secret = "aws_secret_access_key = marker-secret";
    std::fs::write(dir.path().join(Lockfile::FILE_NAME), format!("[default]\n{secret}\n"))
        .expect("write broken lockfile");

    let error = Lockfile::load_wanted_from_dir(dir.path()).expect_err("lockfile must be broken");
    let message = error.to_string();

    assert_eq!(error.code().expect("diagnostic code").to_string(), "ERR_PNPM_BROKEN_LOCKFILE");
    assert!(
        message.starts_with(&format!(
            r#"The lockfile at "{}" is broken: "#,
            dir.path().join(Lockfile::FILE_NAME).display()
        )),
        "unexpected error: {message}",
    );
    assert!(message.contains("(1:1)"), "unexpected error: {message}");
    assert!(!message.contains(secret), "error included lockfile content: {message}");
    assert!(
        std::error::Error::source(&error).is_none(),
        "parse error source could expose lockfile content",
    );
    let report = format!("{:?}", pnpm_diagnostics::miette::Report::new(error));
    assert!(!report.contains(secret), "diagnostic included lockfile content: {report}");
}

/// Heuristic-boundary check: a dropped directory resolution is
/// reconstructed for a pruned `file:` peer-variant, but never for a
/// `file:` tarball.
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
/// version part: Failed to parse version`. The URL is a non-semver
/// version shape and must round-trip through the loader as an
/// `ImporterDepVersion::Regular` with a non-semver version slot,
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

/// Regression test for <https://github.com/pnpm/pnpm/issues/13307>.
#[test]
fn parses_pnpm_10_patched_dependencies_entries() {
    let lockfile_text = text_block! {
        "lockfileVersion: '9.0'"
        ""
        "patchedDependencies:"
        "  is-odd@3.0.1:"
        "    hash: 29572dfbe22f7337d5e2aeab404b7e889550d802c26fa7356730522dd98f4593"
        "    path: patches/is-odd@3.0.1.patch"
        "  is-positive@1.0.0: 6ceb8d5b9e4d6e2f8fca4d7d3f1e0c1b2a3948576d8e2f0c1a4b5d6e7f8091a2"
        ""
        "importers:"
        ""
        "  .: {}"
    };
    let tmp = write_lockfile(lockfile_text);
    let virtual_store_dir = tmp.path().join("node_modules").join(".pacquet");

    let lockfile = Lockfile::load_current_from_virtual_store_dir(&virtual_store_dir)
        .expect("load lockfile with pnpm 10 patchedDependencies")
        .expect("lockfile should be present");

    let patched = lockfile.patched_dependencies.as_ref().expect("patchedDependencies present");
    assert_eq!(
        patched,
        &BTreeMap::from([
            (
                "is-odd@3.0.1".to_string(),
                "29572dfbe22f7337d5e2aeab404b7e889550d802c26fa7356730522dd98f4593".to_string(),
            ),
            (
                "is-positive@1.0.0".to_string(),
                "6ceb8d5b9e4d6e2f8fca4d7d3f1e0c1b2a3948576d8e2f0c1a4b5d6e7f8091a2".to_string(),
            ),
        ]),
    );
}

/// Regression test for <https://github.com/pnpm/pnpm/issues/11775>.
/// An injected workspace package's snapshot can hold a `link:<path>`
/// value in its `dependencies:` map when the dep is a workspace
/// sibling. The shape is valid — a `link:` reference resolves to
/// `null` at use time — so pacquet's parser must accept it.
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
