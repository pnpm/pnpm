use super::{
    super::{PackageProviderError, PackageProviderInputs, build_provider_request},
    helpers::{ENGINE, Fixture, INTEGRITY, key, metadata, snapshot_with_deps, tarball_metadata},
};
use pnpm_lockfile::{
    BinaryArchive, BinaryResolution, BinarySpec, DirectoryResolution, GitResolution,
    LockfileResolution, SnapshotEntry, TarballResolution,
};
use pnpm_patching::ExtendedPatchInfo;
use pretty_assertions::assert_eq;
use std::{collections::HashMap, path::Path};

#[test]
fn empty_graph_skips_the_provider() {
    let fixture = Fixture::new();
    assert!(
        fixture
            .build()
            .expect("build request")
            .is_none(),
    );

    let no_maps = PackageProviderInputs { snapshots: None, packages: None, ..fixture.inputs() };
    assert!(build_provider_request(&no_maps).expect("build request").is_none());
}

#[test]
fn request_contains_closed_graph_over_installed_keys() {
    let mut fixture = Fixture::new()
        .with(
            "foo@1.0.0",
            snapshot_with_deps(&[("bar", "2.0.0"), ("baz", "3.0.0"), ("linked", "link:../linked")]),
            tarball_metadata(),
        )
        .with("bar@2.0.0", SnapshotEntry::default(), tarball_metadata())
        .with(
            "baz@3.0.0",
            SnapshotEntry { optional: true, ..SnapshotEntry::default() },
            tarball_metadata(),
        );
    fixture.skipped.insert_installability(key("baz@3.0.0"));

    let request = fixture.build_json();
    assert_eq!(request["protocol"], 1);
    assert_eq!(request["gcRootDir"], "/workspace/node_modules/.pnpm-nix");

    let nodes = request["nodes"].as_object().expect("nodes object");
    assert!(nodes.contains_key("foo@1.0.0"));
    assert!(nodes.contains_key("bar@2.0.0"));
    assert!(!nodes.contains_key("baz@3.0.0"));

    let foo_node = &nodes["foo@1.0.0"];
    assert_eq!(foo_node["name"], "foo");
    assert_eq!(foo_node["version"], "1.0.0");
    assert_eq!(foo_node["tarball"], "https://registry.example/foo/-/foo-1.0.0.tgz");
    assert_eq!(foo_node["integrity"], INTEGRITY);
    assert_eq!(foo_node["engine"], ENGINE);

    let foo_deps = foo_node["deps"].as_object().expect("foo deps object");
    assert_eq!(foo_deps["bar"]["depPath"], "bar@2.0.0");
    assert_eq!(foo_deps["bar"]["name"], "bar");
    assert!(!foo_deps.contains_key("baz"));
    assert!(!foo_deps.contains_key("linked"));
}

#[test]
fn directory_resolutions_are_sent_as_normalized_absolute_paths() {
    let mut fixture = Fixture::new();
    fixture.lockfile_dir = Path::new("/workspace/root/subdir").to_path_buf();
    let rel_fixture = fixture.with(
        "foo@file:../packages/foo",
        SnapshotEntry::default(),
        metadata(LockfileResolution::Directory(DirectoryResolution {
            directory: "../packages/foo".to_string(),
        })),
    );
    let request = rel_fixture.build_json();
    let node = &request["nodes"]["foo@file:../packages/foo"];
    assert_eq!(node["directory"], "/workspace/root/packages/foo");
}

#[test]
fn git_resolutions_are_sent_as_repo_and_commit() {
    let git_dep_path =
        "foo@git+https://example.com/foo.git#0123456789abcdef0123456789abcdef01234567";
    let fixture = Fixture::new()
        .with(
            git_dep_path,
            SnapshotEntry::default(),
            metadata(LockfileResolution::Git(GitResolution {
                repo: "https://example.com/foo.git".to_string(),
                commit: "0123456789abcdef0123456789abcdef01234567".to_string(),
                integrity: None,
                path: None,
            })),
        );

    let request = fixture.build_json();
    let node = &request["nodes"][git_dep_path];
    assert_eq!(node["git"]["repo"], "https://example.com/foo.git");
    assert_eq!(node["git"]["commit"], "0123456789abcdef0123456789abcdef01234567");
}

#[test]
fn git_resolutions_that_need_prepare_are_rejected() {
    let mut prepare_metadata = metadata(LockfileResolution::Git(GitResolution {
        repo: "https://example.com/foo.git".to_string(),
        commit: "0123456789abcdef0123456789abcdef01234567".to_string(),
        integrity: None,
        path: None,
    }));
    prepare_metadata.prepare = Some(true);
    let fixture = Fixture::new()
        .with("foo@git+https://example.com/foo.git", SnapshotEntry::default(), prepare_metadata);

    let error = fixture.build().expect_err("git prepare must be rejected");
    let message = error.to_string();
    assert!(matches!(error, PackageProviderError::GitPrepareUnsupported { .. }));
    assert!(message.contains(
        "git dependencies that need to be built (prepare) are not supported yet"
    ),);
}

#[test]
fn unsupported_resolutions_are_rejected() {
    let no_integrity = Fixture::new()
        .with(
            "foo@https://example.com/foo.tgz",
            SnapshotEntry::default(),
            metadata(LockfileResolution::Tarball(TarballResolution {
                tarball: "https://example.com/foo.tgz".to_string(),
                integrity: None,
                revision: None,
                git_hosted: None,
                path: None,
            })),
        );
    let error = no_integrity.build().expect_err("tarball without integrity must be rejected");
    assert_eq!(
        error.to_string(),
        "The package provider does not support the resolution of foo@https://example.com/foo.tgz (tarball without integrity)",
    );

    let binary = Fixture::new()
        .with(
            "node@runtime:22.0.0",
            SnapshotEntry::default(),
            metadata(LockfileResolution::Binary(BinaryResolution {
                url: "https://nodejs.org/dist/v22.0.0/node-v22.0.0-linux-x64.tar.gz".to_string(),
                integrity: INTEGRITY.parse().expect("parse integrity"),
                bin: BinarySpec::Single("bin/node".to_string()),
                archive: BinaryArchive::Tarball,
                prefix: None,
            })),
        );
    let error = binary.build().expect_err("binary resolution must be rejected");
    assert_eq!(
        error.to_string(),
        "The package provider does not support the resolution of node@runtime:22.0.0 (binary)",
    );
}

#[test]
fn self_dependencies_are_rejected() {
    let fixture = Fixture::new()
        .with("foo@1.0.0", snapshot_with_deps(&[("foo", "2.0.0")]), tarball_metadata())
        .with("foo@2.0.0", SnapshotEntry::default(), tarball_metadata());

    let error = fixture.build().expect_err("self-dep must be rejected");
    assert!(matches!(error, PackageProviderError::SelfDependency { .. }));
    assert_eq!(
        error.to_string(),
        "The package provider cannot install foo@1.0.0, which depends on a different version of itself",
    );
}

#[test]
fn patch_content_is_sent_inline() {
    let patch_dir = tempfile::tempdir().expect("create temp dir");
    let patch_path = patch_dir.path().join("foo@1.0.0.patch");
    std::fs::write(&patch_path, "--- a/index.js\n+++ b/index.js\n// patched\n")
        .expect("write patch file");
    let mut fixture =
        Fixture::new().with("foo@1.0.0", SnapshotEntry::default(), tarball_metadata());
    fixture.patches = Some(HashMap::from([(
        key("foo@1.0.0"),
        ExtendedPatchInfo {
            hash: "abc123".to_string(),
            patch_file_path: Some(patch_path),
            key: "foo@1.0.0".to_string(),
        },
    )]));

    let request = fixture.build_json();
    let patch = &request["nodes"]["foo@1.0.0"]["patch"];
    assert_eq!(patch["hash"], "abc123");
    assert!(
        patch["content"]
            .as_str()
            .expect("patch content")
            .contains("// patched"),
    );
}

#[test]
fn patch_with_hash_only_is_rejected() {
    let mut fixture =
        Fixture::new().with("foo@1.0.0", SnapshotEntry::default(), tarball_metadata());
    fixture.patches = Some(HashMap::from([(
        key("foo@1.0.0"),
        ExtendedPatchInfo {
            hash: "abc123".to_string(),
            patch_file_path: None,
            key: "foo@1.0.0".to_string(),
        },
    )]));

    let error = fixture.build().expect_err("hash-only patch must be rejected");
    assert!(matches!(error, PackageProviderError::PatchWithoutFile { .. }));
    assert_eq!(
        error.to_string(),
        "The package provider needs the patch file of foo@1.0.0, but only its hash is known",
    );
}
