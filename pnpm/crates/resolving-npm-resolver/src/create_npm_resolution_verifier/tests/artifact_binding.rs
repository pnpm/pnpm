use super::{
    Arc, FAKE_INTEGRITY, Integrity, LockfileResolution, PkgName, REVISION_ONE_DIGEST,
    REVISION_TWO_DIGEST, RegistryResolution, ResolutionVerification, TarballResolution,
    TarballRevision, assert_eq, create_npm_resolution_verifier, ctx, default_opts, fake_integrity,
    now_at, observed_dist_stats_sink, revision_integrity, tarball_resolution,
};
use pnpm_resolving_resolver_base::ResolutionVerifier;

#[tokio::test]
async fn verifies_tarball_url_when_no_policy_active() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let server_url = server.url();
    let packument = serde_json::json!({
        "name": "aged-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "time": { "1.0.0": "2020-01-01T00:00:00.000Z" },
        "versions": {
            "1.0.0": {
                "name": "aged-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": format!("{server_url}/aged-pkg/-/aged-pkg-1.0.0.tgz"),
                }
            }
        }
    });
    let _meta_mock = server
        .mock("GET", "/aged-pkg")
        .with_status(200)
        .with_body(packument.to_string())
        .create_async()
        .await;
    let opts = default_opts(&registry);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://attacker.example/aged-pkg-1.0.0.tgz".to_string(),
        integrity: Some(fake_integrity()),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let name: PkgName = "aged-pkg".parse().expect("parse");
    assert!(verifier.might_verify(&resolution, ctx(&name, "1.0.0")));
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    let ResolutionVerification::Err { code, .. } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "TARBALL_URL_MISMATCH");
}

#[tokio::test]
async fn rejects_a_revision_with_an_unadvertised_integrity() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let packument = serde_json::json!({
        "name": "revision-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "revision-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": revision_integrity(REVISION_TWO_DIGEST).to_string(),
                    "tarball": format!("{registry}-/tarballs/sha512/{REVISION_TWO_DIGEST}"),
                    "revision": 2,
                    "revisions": [{
                        "revision": 1,
                        "integrity": revision_integrity(REVISION_ONE_DIGEST).to_string(),
                        "tarball": format!("{registry}-/tarballs/sha512/{REVISION_ONE_DIGEST}"),
                        "manifest": {},
                    }, {
                        "revision": 2,
                        "integrity": revision_integrity(REVISION_TWO_DIGEST).to_string(),
                        "tarball": format!("{registry}-/tarballs/sha512/{REVISION_TWO_DIGEST}"),
                        "manifest": {},
                    }],
                }
            }
        },
        "time": { "1.0.0": "2020-01-01T00:00:00.000Z" }
    });
    server
        .mock("GET", "/revision-pkg")
        .with_status(200)
        .with_body(packument.to_string())
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(1);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Registry(RegistryResolution {
        integrity: revision_integrity(REVISION_TWO_DIGEST),
        revision: Some(TarballRevision::try_from(1).unwrap()),
    });
    let name = "revision-pkg".parse::<PkgName>().unwrap();
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    let ResolutionVerification::Err { code, .. } = result else {
        panic!("expected revision mismatch, got {result:?}");
    };
    assert_eq!(code, "TARBALL_REVISION_MISMATCH");
}

#[tokio::test]
async fn verify_short_circuits_file_tarball_resolution() {
    let mut opts = default_opts("http://nonexistent.example.invalid/");
    opts.minimum_release_age = Some(60 * 24 * 365);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution =
        tarball_resolution("file:vendor/types__my-cool-lib-v1.0.0.tgz", Some(fake_integrity()));
    let name: PkgName = "@types/my-cool-lib".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// A remote tarball that pins no hash can't be checked against
/// anything once downloaded, so the verifier refuses it before the
/// fetch pass ever sees it. The bogus registry URL is a tripwire: the
/// check is network-free, so no lookup may be attempted.
#[tokio::test]
async fn missing_integrity_is_rejected_before_any_metadata_lookup() {
    let verifier =
        create_npm_resolution_verifier(default_opts("http://nonexistent.example.invalid/"));
    let resolution = tarball_resolution("https://registry.example/foo/-/foo-1.0.0.tgz", None);
    let name: PkgName = "foo".parse().expect("parse");
    assert!(verifier.might_verify(&resolution, ctx(&name, "1.0.0")));
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    assert_eq!(
        result,
        ResolutionVerification::Err {
            code: "MISSING_TARBALL_INTEGRITY",
            reason: r#"has no "integrity" field, so its downloaded tarball cannot be verified"#
                .to_string(),
        },
    );
}

/// `integrity: ''` parses into zero hashes, which pins nothing — the
/// same verdict as an absent field.
#[tokio::test]
async fn empty_integrity_counts_as_missing() {
    let verifier =
        create_npm_resolution_verifier(default_opts("http://nonexistent.example.invalid/"));
    let empty = "".parse::<Integrity>().expect("empty integrity parses");
    let name: PkgName = "foo".parse().expect("parse");
    for resolution in [
        tarball_resolution("https://registry.example/foo/-/foo-1.0.0.tgz", Some(empty.clone())),
        LockfileResolution::Registry(RegistryResolution { integrity: empty, revision: None }),
    ] {
        let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
        let ResolutionVerification::Err { code, .. } = result else {
            panic!("expected Err, got {result:?}");
        };
        assert_eq!(code, "MISSING_TARBALL_INTEGRITY");
    }
}

/// URL-keyed deps carry their spec in the version slot, which skips
/// the registry policies — but not the missing-integrity check, whose
/// verdict doesn't depend on the registry's metadata.
#[tokio::test]
async fn missing_integrity_is_rejected_on_a_non_semver_version() {
    let verifier =
        create_npm_resolution_verifier(default_opts("http://nonexistent.example.invalid/"));
    let tarball = "https://cdn.example/foo/-/foo-1.0.0.tgz";
    let resolution = tarball_resolution(tarball, None);
    let name: PkgName = "foo".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, tarball)).await;
    let ResolutionVerification::Err { code, .. } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "MISSING_TARBALL_INTEGRITY");
}

/// The same URL-keyed entry passes once it pins a hash, without a
/// registry round-trip (the tripwire registry would fail one).
#[tokio::test]
async fn url_keyed_tarball_with_integrity_passes_without_a_lookup() {
    let verifier =
        create_npm_resolution_verifier(default_opts("http://nonexistent.example.invalid/"));
    let tarball = "https://cdn.example/foo/-/foo-1.0.0.tgz";
    let resolution = tarball_resolution(tarball, Some(fake_integrity()));
    let name: PkgName = "foo".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, tarball)).await;
    assert_eq!(result, ResolutionVerification::Ok);
}

/// The exemption is the URL's, not the flag's: a `gitHosted: true`
/// marker on an arbitrary URL, or a git-host URL that isn't pinned to
/// a commit, buys nothing.
#[tokio::test]
async fn integrity_is_required_despite_a_git_hosted_claim() {
    let verifier =
        create_npm_resolution_verifier(default_opts("http://nonexistent.example.invalid/"));
    let name: PkgName = "evil".parse().expect("parse");
    let forged = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://attacker.example/evil-1.0.0.tgz".to_string(),
        integrity: None,
        revision: None,
        git_hosted: Some(true),
        path: None,
    });
    let unpinned =
        tarball_resolution("https://codeload.github.com/kevva/is-negative/tar.gz/main", None);
    for resolution in [forged, unpinned] {
        let version = "https+++attacker.example+evil";
        assert!(verifier.might_verify(&resolution, ctx(&name, version)));
        let result = verifier.verify(&resolution, ctx(&name, version)).await;
        let ResolutionVerification::Err { code, .. } = result else {
            panic!("expected Err, got {result:?}");
        };
        assert_eq!(code, "MISSING_TARBALL_INTEGRITY");
    }
}

/// A registry entry whose pinned tarball URL is not the artifact the
/// registry's metadata lists is rejected before the age check passes it.
/// Guards against a tampered lockfile pairing an aged, trusted
/// name@version with attacker-hosted bytes.
#[tokio::test]
async fn verify_flags_tarball_url_mismatch() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let server_url = server.url();
    let packument = serde_json::json!({
        "name": "aged-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "time": { "1.0.0": "2020-01-01T00:00:00.000Z" },
        "versions": {
            "1.0.0": {
                "name": "aged-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": format!("{server_url}/aged-pkg/-/aged-pkg-1.0.0.tgz"),
                }
            }
        }
    });
    let _meta_mock = server
        .mock("GET", "/aged-pkg")
        .with_status(200)
        .with_body(packument.to_string())
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.minimum_release_age = Some(60 * 24);
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://attacker.example/aged-pkg-1.0.0.tgz".to_string(),
        integrity: Some(fake_integrity()),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let result = verifier
        .verify(&resolution, ctx(&"aged-pkg".parse::<PkgName>().expect("parse"), "1.0.0"))
        .await;
    let ResolutionVerification::Err { code, reason } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "TARBALL_URL_MISMATCH");
    assert!(
        reason.contains("does not match the registry's published metadata"),
        "got reason: {reason}",
    );
}

/// A lockfile URL that differs from the registry metadata only by an
/// explicit default port and the http/https scheme is a benign
/// normalization, not tampering — `same_tarball_url` must canonicalize
/// it away (this is what `canonical_tarball_url`'s URL parse buys over a
/// plain string compare).
#[tokio::test]
async fn tarball_url_default_port_and_scheme_difference_is_a_match() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    // The served metadata lists the artifact on a different host with an
    // explicit default port and the http scheme; the lockfile pins the
    // canonical https/no-port form of the same URL. The host is deliberately
    // not a built-in named registry: one of those would route the metadata
    // fetch to that registry instead of this mock.
    let packument = serde_json::json!({
        "name": "aged-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "time": { "1.0.0": "2020-01-01T00:00:00.000Z" },
        "versions": {
            "1.0.0": {
                "name": "aged-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": "http://cdn.example.test:80/aged-pkg/-/aged-pkg-1.0.0.tgz",
                }
            }
        }
    });
    let _meta_mock = server
        .mock("GET", "/aged-pkg")
        .with_status(200)
        .with_body(packument.to_string())
        .create_async()
        .await;
    let opts = default_opts(&registry);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://cdn.example.test/aged-pkg/-/aged-pkg-1.0.0.tgz".to_string(),
        integrity: Some(fake_integrity()),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let result = verifier
        .verify(&resolution, ctx(&"aged-pkg".parse::<PkgName>().expect("parse"), "1.0.0"))
        .await;
    assert_eq!(result, ResolutionVerification::Ok);
}

#[tokio::test]
async fn binding_check_records_dist_stats_into_the_sink() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let server_url = server.url();
    let tarball_url = format!("{server_url}/acme/-/acme-1.0.0.tgz");
    let packument = serde_json::json!({
        "name": "acme",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "acme",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "tarball": tarball_url,
                    "unpackedSize": 123_456,
                    "fileCount": 42,
                }
            }
        }
    });
    let _meta_mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_body(packument.to_string())
        .create_async()
        .await;

    let sink = observed_dist_stats_sink();
    let mut opts = default_opts(&registry);
    opts.observed_dist_stats = Some(Arc::clone(&sink));
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: tarball_url.clone(),
        integrity: Some(fake_integrity()),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let name: PkgName = "acme".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;

    assert_eq!(result, ResolutionVerification::Ok);
    let recorded = sink
        .get(&("acme".to_string(), "1.0.0".to_string()))
        .map(|entry| *entry.value())
        .expect("stats recorded");
    assert_eq!(recorded.unpacked_size, Some(123_456));
    assert_eq!(recorded.file_count, Some(42));
}

/// The metadata fetch succeeds but does not list the pinned version. That is a
/// genuine verification failure (not a transport error), so it stays
/// `TARBALL_URL_MISMATCH` rather than aborting via `FetchFailed`.
#[tokio::test]
async fn version_absent_from_fetched_metadata_stays_tarball_url_mismatch() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let server_url = server.url();
    let packument = serde_json::json!({
        "name": "present-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "present-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": format!("{server_url}/present-pkg/-/present-pkg-1.0.0.tgz"),
                }
            }
        }
    });
    let _meta_mock = server
        .mock("GET", "/present-pkg")
        .with_status(200)
        .with_body(packument.to_string())
        .create_async()
        .await;

    let opts = default_opts(&registry);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: format!("{server_url}/present-pkg/-/present-pkg-2.0.0.tgz"),
        integrity: Some(fake_integrity()),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let name: PkgName = "present-pkg".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, "2.0.0")).await;

    let ResolutionVerification::Err { code, .. } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "TARBALL_URL_MISMATCH");
}
