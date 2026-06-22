use pacquet_config::Config as PacquetConfig;
use pacquet_resolving_resolver_base::{
    PackageVersionGuard, PackageVersionGuardDecision, PackageVersionGuardFuture,
};
use std::{collections::BTreeMap, sync::Arc};

use super::{
    protocol::{ResolveRequest, ResolveRequestProject},
    resolution_cache_key,
};

fn config() -> PacquetConfig {
    let mut config = PacquetConfig::new();
    config.registry = "https://registry.example.test/".to_string();
    config
}

fn deps(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries.iter().map(|(name, spec)| ((*name).to_string(), (*spec).to_string())).collect()
}

#[derive(Debug)]
struct AllowAllVersions;

impl PackageVersionGuard for AllowAllVersions {
    fn check<'a>(&'a self, _name: &'a str, _version: &'a str) -> PackageVersionGuardFuture<'a> {
        Box::pin(async { Ok(PackageVersionGuardDecision::Allow) })
    }
}

#[test]
fn resolution_cache_key_normalizes_single_project_requests() {
    let top_level = ResolveRequest {
        dependencies: Some(deps(&[("foo", "^1.0.0")])),
        ..ResolveRequest::default()
    };
    let projects = ResolveRequest {
        projects: Some(vec![ResolveRequestProject {
            dir: ".".to_string(),
            dependencies: deps(&[("foo", "^1.0.0")]),
            ..ResolveRequestProject::default()
        }]),
        ..ResolveRequest::default()
    };

    assert_eq!(
        resolution_cache_key(&config(), &top_level),
        resolution_cache_key(&config(), &projects),
    );
}

#[test]
fn resolution_cache_key_changes_with_dependencies_and_policy() {
    let base = ResolveRequest {
        dependencies: Some(deps(&[("foo", "^1.0.0")])),
        ..ResolveRequest::default()
    };
    let different_dep = ResolveRequest {
        dependencies: Some(deps(&[("foo", "^2.0.0")])),
        ..ResolveRequest::default()
    };
    let different_policy = ResolveRequest {
        dependencies: Some(deps(&[("foo", "^1.0.0")])),
        minimum_release_age: Some(60),
        ..ResolveRequest::default()
    };

    let config = config();
    let base_key = resolution_cache_key(&config, &base);

    assert_ne!(base_key, resolution_cache_key(&config, &different_dep));
    assert_ne!(base_key, resolution_cache_key(&config, &different_policy));
}

#[test]
fn a_package_frame_carries_unpacked_size_and_omits_it_when_unknown() {
    use pacquet_package_manager::{ResolutionObserver, ResolvedPackageHint};

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let observer =
        super::StreamObserver { tx, package_version_guard: Some(Arc::new(AllowAllVersions)) };

    let hint = |unpacked_size, file_count| ResolvedPackageHint {
        id: "acme@1.0.0",
        name: "acme",
        version: "1.0.0",
        integrity: "sha512-abc",
        tarball_url: "https://r.test/acme/-/acme-1.0.0.tgz",
        unpacked_size,
        file_count,
    };
    observer.on_resolved(hint(Some(123_456), Some(42)));
    observer.on_resolved(hint(None, None));

    let sized: serde_json::Value =
        serde_json::from_slice(&rx.try_recv().expect("sized frame sent")).unwrap();
    assert_eq!(sized["unpackedSize"], serde_json::json!(123_456));
    assert_eq!(sized["fileCount"], serde_json::json!(42));

    let unsized_frame: serde_json::Value =
        serde_json::from_slice(&rx.try_recv().expect("unsized frame sent")).unwrap();
    dbg!(&unsized_frame);
    assert!(unsized_frame.get("unpackedSize").is_none());
    assert!(unsized_frame.get("fileCount").is_none());
    assert_eq!(unsized_frame["tarball"], serde_json::json!("https://r.test/acme/-/acme-1.0.0.tgz"));
}

#[test]
fn frozen_package_frames_announce_lockfile_tarballs_with_sizes() {
    use pacquet_lockfile::Lockfile;
    use pacquet_resolving_npm_resolver::{DistStats, observed_dist_stats_sink};

    let lockfile: Lockfile = serde_json::from_value(serde_json::json!({
        "lockfileVersion": "9.0",
        "importers": {
            ".": { "dependencies": { "acme": { "specifier": "^1.0.0", "version": "1.0.0" } } }
        },
        "packages": {
            "acme@1.0.0": {
                "resolution": { "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==" }
            },
            "linked-dir@1.0.0": {
                "resolution": { "type": "directory", "directory": "../linked-dir" }
            }
        }
    }))
    .expect("lockfile parses");

    let stats = observed_dist_stats_sink();
    stats.insert(
        ("acme".to_string(), "1.0.0".to_string()),
        DistStats { unpacked_size: Some(123_456), file_count: Some(42) },
    );

    let frames = super::frozen_package_frames(&config(), &lockfile, &stats);
    dbg!(frames.len());
    assert_eq!(frames.len(), 1);

    let frame: serde_json::Value = serde_json::from_slice(&frames[0]).unwrap();
    assert_eq!(frame["type"], serde_json::json!("package"));
    assert_eq!(frame["id"], serde_json::json!("acme@1.0.0"));
    assert_eq!(
        frame["tarball"],
        serde_json::json!("https://registry.example.test/acme/-/acme-1.0.0.tgz"),
    );
    assert_eq!(frame["unpackedSize"], serde_json::json!(123_456));
    assert_eq!(frame["fileCount"], serde_json::json!(42));
}

#[test]
fn osv_checkable_tarball_does_not_trust_git_hosted_flag_or_strict_url_parsing() {
    use pacquet_lockfile::{LockfileResolution, TarballResolution};

    let tarball = |url: &str, git_hosted: Option<bool>| {
        LockfileResolution::Tarball(TarballResolution {
            tarball: url.to_string(),
            integrity: None,
            git_hosted,
            path: None,
        })
    };

    // `gitHosted: true` must not let a normal https registry tarball opt out.
    assert!(super::is_osv_checkable_resolution(&tarball(
        "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz",
        Some(true),
    )));
    // A URL that strict parsing would reject is still scanned when it is http(s).
    assert!(super::is_osv_checkable_resolution(&tarball(
        "https://registry.npmjs.org/foo/-/foo 1.0.0.tgz",
        None,
    )));
    // Mutable git-host archive refs are still checked.
    assert!(super::is_osv_checkable_resolution(&tarball(
        "https://codeload.github.com/foo/bar/tar.gz/abc123",
        Some(false),
    )));
    // Genuinely git-hosted-by-URL tarballs are skipped regardless of the flag.
    assert!(!super::is_osv_checkable_resolution(&tarball(
        "https://codeload.github.com/foo/bar/tar.gz/0123456789abcdef0123456789abcdef01234567",
        Some(false),
    )));
    // Non-http schemes are skipped.
    assert!(!super::is_osv_checkable_resolution(&tarball("file:../foo.tgz", None)));
}

#[test]
fn tarball_url_version_extracts_conventional_names_only() {
    use super::tarball_url_version;

    assert_eq!(tarball_url_version("https://r/foo/-/foo-1.2.3.tgz", "foo"), Some("1.2.3"));
    // Scoped packages name the tarball file with the unscoped name.
    assert_eq!(tarball_url_version("https://r/@s/foo/-/foo-1.2.3.tgz", "@s/foo"), Some("1.2.3"));
    // Query/fragment are stripped; prerelease/build keep working.
    assert_eq!(tarball_url_version("https://r/foo/-/foo-1.2.3.tgz?x=1", "foo"), Some("1.2.3"));
    assert_eq!(
        tarball_url_version("https://r/foo/-/foo-1.2.3-beta.1.tgz", "foo"),
        Some("1.2.3-beta.1"),
    );
    // Suffix matching is case-insensitive and covers `.tar.gz`, so a
    // tampered lockfile can't dodge the cross-check with a variant.
    assert_eq!(tarball_url_version("https://r/foo/-/foo-1.2.3.TGZ", "foo"), Some("1.2.3"));
    assert_eq!(tarball_url_version("https://r/foo/-/foo-1.2.3.tar.gz", "foo"), Some("1.2.3"));
    // Non-conventional naming yields None (fall back, don't misjudge).
    assert_eq!(tarball_url_version("https://r/weird.tgz", "foo"), None);
    assert_eq!(tarball_url_version("https://r/foo/-/foo.tgz", "foo"), None);
}
