use super::{
    PackageProviderError, PackageProviderInputs, ProviderRequestBundle, ProviderResponse,
    build_provider_request, parse_provider_response, validate_provider_response,
};
use crate::SkippedSnapshots;
use pacquet_config::Config;
use pacquet_lockfile::{
    BinaryArchive, BinaryResolution, BinarySpec, DirectoryResolution, GitResolution,
    LockfileResolution, PackageKey, PackageMetadata, PkgName, RegistryResolution, SnapshotDepRef,
    SnapshotEntry, TarballResolution,
};
use pacquet_patching::ExtendedPatchInfo;
use pretty_assertions::assert_eq;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::OnceLock,
};

const INTEGRITY: &str = "sha512-AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8gISIjJCUmJygpKissLS4vMDEyMzQ1Njc4OTo7PD0+Pw==";
const ENGINE: &str = "linux;x64;node20";

fn static_config() -> &'static Config {
    static CONFIG: OnceLock<&'static Config> = OnceLock::new();
    CONFIG.get_or_init(|| Config::default().leak())
}

fn key(dep_path: &str) -> PackageKey {
    dep_path.parse().expect("parse package key")
}

fn metadata(resolution: LockfileResolution) -> PackageMetadata {
    PackageMetadata {
        resolution,
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

fn tarball_metadata() -> PackageMetadata {
    metadata(LockfileResolution::Tarball(TarballResolution {
        tarball: "https://registry.example/foo/-/foo-1.0.0.tgz".to_string(),
        integrity: Some(INTEGRITY.parse().expect("parse integrity")),
        git_hosted: None,
        path: None,
    }))
}

fn snapshot_with_deps(deps: &[(&str, &str)]) -> SnapshotEntry {
    SnapshotEntry {
        dependencies: (!deps.is_empty()).then(|| {
            deps.iter()
                .map(|(alias, dep_ref)| {
                    (
                        PkgName::parse(*alias).expect("parse alias"),
                        dep_ref.parse::<SnapshotDepRef>().expect("parse dep ref"),
                    )
                })
                .collect()
        }),
        ..SnapshotEntry::default()
    }
}

struct Fixture {
    snapshots: HashMap<PackageKey, SnapshotEntry>,
    packages: HashMap<PackageKey, PackageMetadata>,
    skipped: SkippedSnapshots,
    patches: Option<HashMap<PackageKey, ExtendedPatchInfo>>,
    lockfile_dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        Fixture {
            snapshots: HashMap::new(),
            packages: HashMap::new(),
            skipped: SkippedSnapshots::new(),
            patches: None,
            lockfile_dir: PathBuf::from("/workspace"),
        }
    }

    fn with(mut self, dep_path: &str, snapshot: SnapshotEntry, meta: PackageMetadata) -> Self {
        let full_key = key(dep_path);
        self.packages.insert(full_key.without_peer(), meta);
        self.snapshots.insert(full_key, snapshot);
        self
    }

    fn inputs(&self) -> PackageProviderInputs<'_> {
        PackageProviderInputs {
            package_provider: "/provider",
            lockfile_dir: &self.lockfile_dir,
            snapshots: Some(&self.snapshots),
            packages: Some(&self.packages),
            skipped: &self.skipped,
            patches: self.patches.as_ref(),
            engine: Some(ENGINE),
            config: static_config(),
        }
    }

    fn build(&self) -> Result<Option<ProviderRequestBundle>, PackageProviderError> {
        build_provider_request(&self.inputs())
    }

    fn build_json(&self) -> serde_json::Value {
        let bundle = self.build().expect("build request").expect("non-empty request");
        serde_json::to_value(&bundle.request).expect("serialize request")
    }
}

#[test]
fn empty_graph_skips_the_provider() {
    let fixture = Fixture::new();
    assert!(fixture.build().expect("build request").is_none());

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
    // `baz` was skipped by the installability pass: it must be absent
    // from the request and the `foo -> baz` edge must be dropped.
    fixture.skipped.insert_installability(key("baz@3.0.0"));

    let request = fixture.build_json();
    dbg!(&request);
    assert_eq!(request["protocol"], 1);
    assert_eq!(
        request["gcRootDir"].as_str().expect("gcRootDir string"),
        Path::new("/workspace").join("node_modules").join(".pnpm-nix").to_string_lossy(),
    );
    let nodes = request["nodes"].as_object().expect("nodes object");
    assert_eq!(nodes.len(), 2);
    assert!(nodes.contains_key("foo@1.0.0"));
    assert!(nodes.contains_key("bar@2.0.0"));
    let foo_deps = request["nodes"]["foo@1.0.0"]["deps"].as_object().expect("deps object");
    assert_eq!(foo_deps.len(), 1);
    assert_eq!(foo_deps["bar"]["depPath"], "bar@2.0.0");
    assert_eq!(foo_deps["bar"]["name"], "bar");
    assert_eq!(request["nodes"]["foo@1.0.0"]["engine"], ENGINE);
    assert_eq!(request["nodes"]["foo@1.0.0"]["version"], "1.0.0");
}

#[test]
fn optional_is_emitted_only_when_true() {
    let fixture =
        Fixture::new().with("foo@1.0.0", SnapshotEntry::default(), tarball_metadata()).with(
            "bar@2.0.0",
            SnapshotEntry { optional: true, ..SnapshotEntry::default() },
            tarball_metadata(),
        );

    let request = fixture.build_json();
    dbg!(&request);
    assert_eq!(request["nodes"]["bar@2.0.0"]["optional"], true);
    assert!(request["nodes"]["foo@1.0.0"].get("optional").is_none());
}

#[test]
fn tarball_and_registry_resolutions_carry_tarball_and_integrity() {
    let fixture =
        Fixture::new().with("foo@1.0.0", SnapshotEntry::default(), tarball_metadata()).with(
            "bar@2.0.0",
            SnapshotEntry::default(),
            metadata(LockfileResolution::Registry(RegistryResolution {
                integrity: INTEGRITY.parse().expect("parse integrity"),
            })),
        );

    let request = fixture.build_json();
    dbg!(&request);
    assert_eq!(
        request["nodes"]["foo@1.0.0"]["tarball"],
        "https://registry.example/foo/-/foo-1.0.0.tgz",
    );
    assert_eq!(request["nodes"]["foo@1.0.0"]["integrity"], INTEGRITY);
    // Registry resolutions have their tarball URL derived from the
    // configured registry, mirroring `pkgSnapshotToResolution`.
    let registry_tarball =
        request["nodes"]["bar@2.0.0"]["tarball"].as_str().expect("derived tarball url");
    assert!(registry_tarball.ends_with("/bar/-/bar-2.0.0.tgz"), "got {registry_tarball}");
    assert_eq!(request["nodes"]["bar@2.0.0"]["integrity"], INTEGRITY);
}

#[test]
fn directory_resolutions_are_sent_as_absolute_paths() {
    let fixture = Fixture::new().with(
        "local-pkg@file:local-pkg",
        SnapshotEntry::default(),
        metadata(LockfileResolution::Directory(DirectoryResolution {
            directory: "local-pkg".to_string(),
        })),
    );

    let request = fixture.build_json();
    dbg!(&request);
    let node = &request["nodes"]["local-pkg@file:local-pkg"];
    assert_eq!(
        node["directory"].as_str().expect("directory string"),
        pacquet_fs::lexical_normalize(&Path::new("/workspace").join("local-pkg")).to_string_lossy(),
    );
    assert!(node.get("tarball").is_none());
    // `file:` depPaths carry no semver, so no version is sent.
    assert!(node.get("version").is_none());
}

#[test]
fn git_resolutions_are_sent_by_repo_and_commit() {
    let git_dep_path = "foo@git+https://example.com/foo.git";
    let fixture = Fixture::new().with(
        git_dep_path,
        SnapshotEntry::default(),
        metadata(LockfileResolution::Git(GitResolution {
            repo: "https://example.com/foo.git".to_string(),
            commit: "0123456789abcdef0123456789abcdef01234567".to_string(),
            path: None,
        })),
    );

    let request = fixture.build_json();
    dbg!(&request);
    let node = &request["nodes"][git_dep_path];
    assert_eq!(node["git"]["repo"], "https://example.com/foo.git");
    assert_eq!(node["git"]["commit"], "0123456789abcdef0123456789abcdef01234567");
}

#[test]
fn git_resolutions_that_need_prepare_are_rejected() {
    let mut prepare_metadata = metadata(LockfileResolution::Git(GitResolution {
        repo: "https://example.com/foo.git".to_string(),
        commit: "0123456789abcdef0123456789abcdef01234567".to_string(),
        path: None,
    }));
    prepare_metadata.prepare = Some(true);
    let fixture = Fixture::new().with(
        "foo@git+https://example.com/foo.git",
        SnapshotEntry::default(),
        prepare_metadata,
    );

    let error = fixture.build().expect_err("git prepare must be rejected");
    let message = error.to_string();
    eprintln!("MESSAGE:\n{message}\n");
    assert!(matches!(error, PackageProviderError::GitPrepareUnsupported { .. }));
    assert!(
        message.contains("git dependencies that need to be built (prepare) are not supported yet"),
    );
}

#[test]
fn unsupported_resolutions_are_rejected() {
    let no_integrity = Fixture::new().with(
        "foo@https://example.com/foo.tgz",
        SnapshotEntry::default(),
        metadata(LockfileResolution::Tarball(TarballResolution {
            tarball: "https://example.com/foo.tgz".to_string(),
            integrity: None,
            git_hosted: None,
            path: None,
        })),
    );
    let error = no_integrity.build().expect_err("tarball without integrity must be rejected");
    let message = error.to_string();
    eprintln!("MESSAGE:\n{message}\n");
    assert_eq!(
        message,
        "The package provider does not support the resolution of foo@https://example.com/foo.tgz (tarball without integrity)",
    );

    let binary = Fixture::new().with(
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
    let error = binary.build().expect_err("binary resolutions must be rejected");
    let message = error.to_string();
    eprintln!("MESSAGE:\n{message}\n");
    assert!(matches!(error, PackageProviderError::UnsupportedResolution { kind: "binary", .. }));
}

#[test]
fn depending_on_a_different_version_of_itself_is_rejected() {
    let fixture = Fixture::new()
        .with("foo@1.0.0", snapshot_with_deps(&[("foo", "foo@2.0.0")]), tarball_metadata())
        .with("foo@2.0.0", SnapshotEntry::default(), tarball_metadata());

    let error = fixture.build().expect_err("self dependency must be rejected");
    let message = error.to_string();
    eprintln!("MESSAGE:\n{message}\n");
    assert!(matches!(error, PackageProviderError::SelfDependency { .. }));
    assert_eq!(
        message,
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
    dbg!(&request);
    let patch = &request["nodes"]["foo@1.0.0"]["patch"];
    assert_eq!(patch["hash"], "abc123");
    assert!(patch["content"].as_str().expect("patch content").contains("// patched"));
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
    let message = error.to_string();
    eprintln!("MESSAGE:\n{message}\n");
    assert!(matches!(error, PackageProviderError::PatchWithoutFile { .. }));
    assert_eq!(
        message,
        "The package provider needs the patch file of foo@1.0.0, but only its hash is known",
    );
}

/// A two-node bundle (`foo` required, `bar` optional) shared by the
/// response-validation tests below.
fn two_node_bundle() -> ProviderRequestBundle {
    Fixture::new()
        .with("foo@1.0.0", SnapshotEntry::default(), tarball_metadata())
        .with(
            "bar@2.0.0",
            SnapshotEntry { optional: true, ..SnapshotEntry::default() },
            tarball_metadata(),
        )
        .build()
        .expect("build request")
        .expect("non-empty request")
}

fn response(paths: &[(&str, &str)], skipped: &[&str]) -> ProviderResponse {
    ProviderResponse {
        protocol: Some(1),
        paths: Some(
            paths.iter().map(|(dep_path, dir)| (dep_path.to_string(), dir.to_string())).collect(),
        ),
        skipped: Some(skipped.iter().map(ToString::to_string).collect()),
    }
}

#[test]
fn response_must_be_valid_json() {
    let error = parse_provider_response("/provider", b"not json")
        .expect_err("invalid JSON must be rejected");
    let message = error.to_string();
    eprintln!("MESSAGE:\n{message}\n");
    assert_eq!(message, r#"The package provider at "/provider" did not return valid JSON"#);
}

#[test]
fn response_protocol_must_match() {
    let error = parse_provider_response("/provider", br#"{"protocol":2,"paths":{}}"#)
        .expect_err("wrong protocol must be rejected");
    let message = error.to_string();
    eprintln!("MESSAGE:\n{message}\n");
    assert_eq!(
        message,
        r#"The package provider at "/provider" returned an unsupported response (protocol 2)"#,
    );

    let error = parse_provider_response("/provider", br#"{"paths":{}}"#)
        .expect_err("missing protocol must be rejected");
    let message = error.to_string();
    eprintln!("MESSAGE:\n{message}\n");
    assert_eq!(
        message,
        r#"The package provider at "/provider" returned an unsupported response (protocol missing)"#,
    );

    let error = parse_provider_response("/provider", br#"{"protocol":1}"#)
        .expect_err("missing paths must be rejected");
    let message = error.to_string();
    eprintln!("MESSAGE:\n{message}\n");
    assert!(matches!(error, PackageProviderError::UnsupportedResponse { .. }));
}

#[test]
fn skipping_a_non_optional_package_is_rejected() {
    let bundle = two_node_bundle();
    let error = validate_provider_response(
        &bundle,
        response(&[("bar@2.0.0", "/store/bar")], &["foo@1.0.0"]),
    )
    .expect_err("skipping a required package must be rejected");
    let message = error.to_string();
    eprintln!("MESSAGE:\n{message}\n");
    assert_eq!(
        message,
        "The package provider skipped foo@1.0.0, which is not an optional dependency",
    );

    let error = validate_provider_response(&bundle, response(&[], &["ghost@9.9.9"]))
        .expect_err("skipping an unknown package must be rejected");
    let message = error.to_string();
    eprintln!("MESSAGE:\n{message}\n");
    assert!(matches!(error, PackageProviderError::SkippedNonOptional { .. }));
}

#[test]
fn a_missing_path_for_an_installed_package_is_rejected() {
    let bundle = two_node_bundle();
    let error = validate_provider_response(&bundle, response(&[("bar@2.0.0", "/store/bar")], &[]))
        .expect_err("a missing path must be rejected");
    let message = error.to_string();
    eprintln!("MESSAGE:\n{message}\n");
    assert_eq!(message, "The package provider returned no path for foo@1.0.0");
}

#[test]
fn skipped_optionals_are_excluded_from_the_path_requirement() {
    let bundle = two_node_bundle();
    let output = validate_provider_response(
        &bundle,
        response(&[("foo@1.0.0", "/store/foo")], &["bar@2.0.0"]),
    )
    .expect("skipping an optional package is valid");
    dbg!(&output);
    assert_eq!(output.skipped, vec![key("bar@2.0.0")]);
    assert_eq!(output.paths, HashMap::from([(key("foo@1.0.0"), PathBuf::from("/store/foo"))]));
}
