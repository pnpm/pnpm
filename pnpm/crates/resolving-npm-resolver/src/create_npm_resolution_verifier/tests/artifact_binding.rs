use super::{
    Arc, FAKE_INTEGRITY, Integrity, LockfileResolution, PkgName, PlatformAssetResolution,
    REVISION_ONE_DIGEST, REVISION_TWO_DIGEST, REVISION_ZERO_DIGEST, RegistryResolution,
    ResolutionVerification, TarballResolution, TarballRevision, VariationsResolution, assert_eq,
    create_npm_resolution_verifier, ctx, default_opts, fake_integrity, now_at,
    observed_dist_stats_sink, revision_integrity, tarball_resolution,
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
                    "tarball": format!("{registry}revision-pkg@1.0.0+r2"),
                    "revision": 2,
                    "revisions": [{
                        "revision": 0,
                        "integrity": revision_integrity(REVISION_ZERO_DIGEST).to_string(),
                        "tarball": format!("{registry}revision-pkg@1.0.0+r0"),
                        "manifest": {},
                    }, {
                        "revision": 1,
                        "integrity": revision_integrity(REVISION_ONE_DIGEST).to_string(),
                        "tarball": format!("{registry}revision-pkg@1.0.0+r1"),
                        "manifest": {},
                    }, {
                        "revision": 2,
                        "integrity": revision_integrity(REVISION_TWO_DIGEST).to_string(),
                        "tarball": format!("{registry}revision-pkg@1.0.0+r2"),
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
    opts.release_age.minimum_minutes = Some(1);
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
    opts.release_age.minimum_minutes = Some(60 * 24 * 365);
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
    opts.release_age.minimum_minutes = Some(60 * 24);
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: "https://attacker.example/aged-pkg-1.0.0.tgz".to_string(),
        integrity: Some(fake_integrity()),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let result =
        verifier.verify(&resolution, ctx(&"aged-pkg".parse::<PkgName>().expect("parse"), "1.0.0"))
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
    let result =
        verifier.verify(&resolution, ctx(&"aged-pkg".parse::<PkgName>().expect("parse"), "1.0.0"))
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
    opts.artifacts.observed_stats = Some(Arc::clone(&sink));
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

#[tokio::test]
async fn verifies_variations_resolution_matching_tarball_url() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let server_url = server.url();
    let packument = serde_json::json!({
        "name": "variations-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "time": { "1.0.0": "2020-01-01T00:00:00.000Z" },
        "versions": {
            "1.0.0": {
                "name": "variations-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": format!("{server_url}/variations-pkg/-/variations-pkg-1.0.0.tgz"),
                }
            }
        }
    });
    let _meta_mock = server
        .mock("GET", "/variations-pkg")
        .with_status(200)
        .with_body(packument.to_string())
        .create_async()
        .await;

    let opts = default_opts(&registry);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Variations(VariationsResolution {
        revision: None,
        variants: vec![PlatformAssetResolution {
            resolution: LockfileResolution::Tarball(TarballResolution {
                tarball: format!("{server_url}/variations-pkg/-/variations-pkg-1.0.0.tgz"),
                integrity: Some(fake_integrity()),
                revision: None,
                git_hosted: None,
                path: None,
            }),
            targets: vec![],
        }],
    });
    let name: PkgName = "variations-pkg".parse().expect("parse");
    assert!(verifier.might_verify(&resolution, ctx(&name, "1.0.0")));
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    assert_eq!(result, ResolutionVerification::Ok);
}

#[tokio::test]
async fn rejects_variations_resolution_with_mismatched_tarball_url() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let server_url = server.url();
    let packument = serde_json::json!({
        "name": "variations-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "time": { "1.0.0": "2020-01-01T00:00:00.000Z" },
        "versions": {
            "1.0.0": {
                "name": "variations-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": format!("{server_url}/variations-pkg/-/variations-pkg-1.0.0.tgz"),
                }
            }
        }
    });
    let _meta_mock = server
        .mock("GET", "/variations-pkg")
        .with_status(200)
        .with_body(packument.to_string())
        .create_async()
        .await;

    let opts = default_opts(&registry);
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Variations(VariationsResolution {
        revision: None,
        variants: vec![PlatformAssetResolution {
            resolution: LockfileResolution::Tarball(TarballResolution {
                tarball: "https://attacker.example/variations-pkg-1.0.0.tgz".to_string(),
                integrity: Some(fake_integrity()),
                revision: None,
                git_hosted: None,
                path: None,
            }),
            targets: vec![],
        }],
    });
    let name: PkgName = "variations-pkg".parse().expect("parse");
    assert!(verifier.might_verify(&resolution, ctx(&name, "1.0.0")));
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    let ResolutionVerification::Err { code, .. } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "TARBALL_URL_MISMATCH");
}

#[tokio::test]
async fn rejects_variations_resolution_with_missing_integrity() {
    let opts = default_opts("https://registry.example/");
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Variations(VariationsResolution {
        revision: None,
        variants: vec![PlatformAssetResolution {
            resolution: LockfileResolution::Tarball(TarballResolution {
                tarball: "https://registry.example/pkg-1.0.0.tgz".to_string(),
                integrity: None,
                revision: None,
                git_hosted: None,
                path: None,
            }),
            targets: vec![],
        }],
    });
    let name: PkgName = "variations-pkg".parse().expect("parse");
    assert!(verifier.might_verify(&resolution, ctx(&name, "1.0.0")));
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    let ResolutionVerification::Err { code, .. } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "MISSING_TARBALL_INTEGRITY");
}

/// A host that no other variant matches installs the variant that targets
/// `any`, so it is verified like every variant.
#[tokio::test]
async fn rejects_variations_resolution_with_an_unverifiable_any_variant() {
    let opts = default_opts("https://registry.example/");
    let verifier = create_npm_resolution_verifier(opts);
    let resolution = LockfileResolution::Variations(VariationsResolution {
        revision: None,
        variants: vec![pnpm_lockfile::PlatformAssetResolution::any(LockfileResolution::Tarball(
            TarballResolution {
                tarball: "https://registry.example/pkg-1.0.0.tgz".to_string(),
                integrity: None,
                revision: None,
                git_hosted: None,
                path: None,
            },
        ))],
    });
    let name: PkgName = "variations-pkg".parse().expect("parse");
    assert!(verifier.might_verify(&resolution, ctx(&name, "1.0.0")));
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    let ResolutionVerification::Err { code, .. } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "MISSING_TARBALL_INTEGRITY");
}

/// A lockfile that keeps tarball URLs writes a variant at its own route,
/// which the version's `dist.tarball` (the primary's) does not name.
#[tokio::test]
async fn verifies_a_variant_tarball_url_on_its_own_route() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let server_url = server.url();
    let packument = serde_json::json!({
        "name": "variations-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "time": { "1.0.0": "2020-01-01T00:00:00.000Z" },
        "versions": {
            "1.0.0": {
                "name": "variations-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": format!("{server_url}/variations-pkg/-/variations-pkg-1.0.0.tgz"),
                }
            }
        }
    });
    let _meta_mock = server
        .mock("GET", "/variations-pkg")
        .with_status(200)
        .with_body(packument.to_string())
        .create_async()
        .await;

    let verifier = create_npm_resolution_verifier(default_opts(&registry));
    let variant = |tarball: String| PlatformAssetResolution {
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball,
            integrity: Some(fake_integrity()),
            revision: None,
            git_hosted: None,
            path: None,
        }),
        targets: vec![pnpm_lockfile::PlatformAssetTarget {
            os: "linux".to_string(),
            cpu: "x64".to_string(),
            ..Default::default()
        }],
    };
    let tag = variant(String::new()).variant_tag().expect("a variant tag");
    let name: PkgName = "variations-pkg".parse().expect("parse");

    let on_route = LockfileResolution::Variations(VariationsResolution {
        revision: None,
        variants: vec![variant(format!("{server_url}/variations-pkg@1.0.0/variants/{tag}"))],
    });
    let result = verifier.verify(&on_route, ctx(&name, "1.0.0")).await;
    assert_eq!(result, ResolutionVerification::Ok);

    let other_version = LockfileResolution::Variations(VariationsResolution {
        revision: None,
        variants: vec![variant(format!("{server_url}/variations-pkg@2.0.0/variants/{tag}"))],
    });
    let result = verifier.verify(&other_version, ctx(&name, "1.0.0")).await;
    let ResolutionVerification::Err { code, .. } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "TARBALL_URL_MISMATCH");
}

/// A variant on the route of a registry other than the default one is
/// fetched from that registry, so the policies read its metadata.
#[tokio::test]
async fn checks_a_variant_on_its_route_against_the_registry_it_is_fetched_from() {
    let mut server = mockito::Server::new_async().await;
    let server_url = server.url();
    let _attestation_mock = server
        .mock("GET", "/variants-registry/-/npm/v1/attestations/variations-pkg@1.0.0")
        .with_status(404)
        .create_async()
        .await;
    let metadata_mock = server
        .mock("GET", "/variants-registry/variations-pkg")
        .with_status(200)
        .with_body(
            super::min_age_packument_json("variations-pkg", "1.0.0", "2025-11-30T22:00:00.000Z")
                .to_string(),
        )
        .expect_at_least(1)
        .create_async()
        .await;

    let mut opts = default_opts("http://public.example.com/");
    opts.registries.insert("@other".to_string(), format!("{server_url}/variants-registry/"));
    opts.release_age.minimum_minutes = Some(60 * 24);
    opts.now = Some(now_at("2025-12-01T00:00:00Z"));
    let verifier = create_npm_resolution_verifier(opts);
    let mut variant = PlatformAssetResolution {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: fake_integrity(),
            revision: None,
        }),
        targets: vec![pnpm_lockfile::PlatformAssetTarget {
            os: "linux".to_string(),
            cpu: "x64".to_string(),
            ..Default::default()
        }],
    };
    let tag = variant.variant_tag().expect("a variant tag");
    variant.resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: format!("{server_url}/variants-registry/variations-pkg@1.0.0/variants/{tag}"),
        integrity: Some(fake_integrity()),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let resolution = LockfileResolution::Variations(VariationsResolution {
        revision: None,
        variants: vec![variant],
    });
    let name: PkgName = "variations-pkg".parse().expect("parse");
    let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
    let ResolutionVerification::Err { code, .. } = result else {
        panic!("expected Err, got {result:?}");
    };
    assert_eq!(code, "MINIMUM_RELEASE_AGE_VIOLATION");
    metadata_mock.assert_async().await;
}

/// A variant's URL matches its route under the same equivalences as a
/// primary's URL matches `dist.tarball`: scheme and an encoded scope
/// separator do not matter.
#[tokio::test]
async fn matches_a_variant_route_up_to_scheme_and_scope_encoding() {
    let verifier = create_npm_resolution_verifier(default_opts("https://registry.example/"));
    let targets = vec![pnpm_lockfile::PlatformAssetTarget {
        os: "linux".to_string(),
        cpu: "x64".to_string(),
        ..Default::default()
    }];
    let tag = PlatformAssetResolution {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: fake_integrity(),
            revision: None,
        }),
        targets: targets.clone(),
    }
    .variant_tag()
    .expect("a variant tag");
    let name: PkgName = "@scope/variations-pkg".parse().expect("parse");
    for tarball in [
        format!("http://registry.example/@scope/variations-pkg@1.0.0/variants/{tag}"),
        format!("https://registry.example/@scope%2fvariations-pkg@1.0.0/variants/{tag}"),
    ] {
        let resolution = LockfileResolution::Variations(VariationsResolution {
            revision: None,
            variants: vec![PlatformAssetResolution {
                resolution: LockfileResolution::Tarball(TarballResolution {
                    tarball: tarball.clone(),
                    integrity: Some(fake_integrity()),
                    revision: None,
                    git_hosted: None,
                    path: None,
                }),
                targets: targets.clone(),
            }],
        });
        let result = verifier.verify(&resolution, ctx(&name, "1.0.0")).await;
        assert_eq!(result, ResolutionVerification::Ok, "{tarball}");
    }
}
