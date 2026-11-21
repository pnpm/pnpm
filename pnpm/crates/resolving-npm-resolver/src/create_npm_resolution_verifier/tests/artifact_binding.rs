use super::{
    Arc,
    FAKE_INTEGRITY,
    Integrity,
    LockfileResolution,
    PkgName,
    PlatformAssetResolution,
    REVISION_ONE_DIGEST,
    REVISION_TWO_DIGEST,
    REVISION_ZERO_DIGEST,
    RegistryResolution,
    ResolutionVerification,
    TarballResolution,
    TarballRevision,
    VariationsResolution,
    assert_eq,
    create_npm_resolution_verifier,
    ctx,
    default_opts,
    fake_integrity,
    now_at,
    observed_dist_stats_sink,
    revision_integrity,
    tarball_resolution,
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

/// `integrity: ''` parses into zero hashes, which pins nothing, the
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
/// the registry policies, but not the missing-integrity check, whose
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
/// normalization, not tampering, `same_tarball_url` must canonicalize
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
        .get(&crate::observed_dist_stats_key(&registry, "acme".to_owned(), "1.0.0".to_owned()))
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

#[tokio::test]
async fn published_tarball_mapper_accepts_routed_split_domain_url_and_redacts_mismatch() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let packument = serde_json::json!({
        "name": "@corp/split",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "@corp/split",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "tarball": "https://cdn.split.example/@corp/split/split-1.0.0.tgz?X-Amz-Signature=EXAMPLE_PRESIGNED_TOKEN",
                }
            }
        }
    });
    let _meta_mock = server
        .mock("GET", "/@corp%2Fsplit")
        .with_body(packument.to_string())
        .expect_at_least(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.artifacts.published_tarball_mapper =
        Some(std::sync::Arc::new(move |_, pkg, ver, lockfile_url, _| {
            let routed = format!("https://pnpr.example/npm/~corp/{pkg}/-/split-{ver}.tgz");
            crate::MappedTarballUrl {
                accepted_routed_url: lockfile_url
                    .starts_with("https://pnpr.example/npm/~")
                    .then(|| routed.clone()),
                display_url: routed,
                accept_published_url: false,
            }
        }));
    let verifier = create_npm_resolution_verifier(opts);
    let name: PkgName = "@corp/split".parse().expect("parse");
    let ok = tarball_resolution(
        "https://pnpr.example/npm/~corp/@corp/split/-/split-1.0.0.tgz",
        Some(fake_integrity()),
    );
    assert_eq!(verifier.verify(&ok, ctx(&name, "1.0.0")).await, ResolutionVerification::Ok);

    // Direct synthetic upstream URLs and raw presigned CDN URLs are rejected.
    for unrouted in [
        format!("{}/@corp/split/-/split-1.0.0.tgz", server.url()),
        "https://cdn.split.example/@corp/split/split-1.0.0.tgz?X-Amz-Signature=EXAMPLE_PRESIGNED_TOKEN".to_string(),
    ] {
        let candidate = tarball_resolution(&unrouted, Some(fake_integrity()));
        let ResolutionVerification::Err { code, reason } =
            verifier.verify(&candidate, ctx(&name, "1.0.0")).await
        else {
            panic!("expected unrouted URL {unrouted} to be rejected");
        };
        assert_eq!(code, "TARBALL_URL_MISMATCH");
        for secret in ["cdn.split.example", "EXAMPLE_PRESIGNED_TOKEN"] {
            assert!(!reason.contains(secret), "{secret} leaked in {reason}");
        }
    }

    let signed_bad = tarball_resolution(
        &format!(
            "{}/@corp/split/-/wrong-1.0.0.tgz?X-Amz-Signature=LOCKFILE_PRESIGNED_SECRET#frag",
            server.url(),
        ),
        Some(fake_integrity()),
    );
    let ResolutionVerification::Err { code, reason } =
        verifier.verify(&signed_bad, ctx(&name, "1.0.0")).await
    else {
        panic!("expected mismatch");
    };
    assert_eq!(code, "TARBALL_URL_MISMATCH");
    for secret in ["cdn.split.example", "EXAMPLE_PRESIGNED_TOKEN", "LOCKFILE_PRESIGNED_SECRET"] {
        assert!(!reason.contains(secret), "{secret} leaked in {reason}");
    }
}

#[tokio::test]
async fn published_tarball_mapper_accepts_both_published_and_sanitized_public_urls() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let packument = serde_json::json!({
        "name": "public-query-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "public-query-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "tarball": "https://cdn.public.example/public-query-pkg-1.0.0.tgz?download=1",
                }
            }
        }
    });
    let _meta_mock = server
        .mock("GET", "/public-query-pkg")
        .with_body(packument.to_string())
        .expect_at_least(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    opts.artifacts.published_tarball_mapper = Some(std::sync::Arc::new(|_, _, _, _, published| {
        let sanitized = published
            .split(['?', '#'])
            .next()
            .unwrap_or(published)
            .to_string();
        crate::MappedTarballUrl {
            accepted_routed_url: (sanitized != published).then(|| sanitized.clone()),
            display_url: sanitized,
            accept_published_url: true,
        }
    }));
    let verifier = create_npm_resolution_verifier(opts);
    let name: PkgName = "public-query-pkg".parse().expect("parse");
    for allowed in [
        "https://cdn.public.example/public-query-pkg-1.0.0.tgz?download=1",
        "https://cdn.public.example/public-query-pkg-1.0.0.tgz",
    ] {
        let candidate = tarball_resolution(allowed, Some(fake_integrity()));
        assert_eq!(
            verifier.verify(&candidate, ctx(&name, "1.0.0")).await,
            ResolutionVerification::Ok,
            "{allowed} should pass",
        );
    }

    // Any other query string matches neither `published` nor `sanitized`.
    let other_query = tarball_resolution(
        "https://cdn.public.example/public-query-pkg-1.0.0.tgz?download=2",
        Some(fake_integrity()),
    );
    assert!(matches!(
        verifier.verify(&other_query, ctx(&name, "1.0.0")).await,
        ResolutionVerification::Err { code: "TARBALL_URL_MISMATCH", .. },
    ));
}

#[tokio::test]
async fn undeclared_named_registry_is_rejected_even_without_tarball_url_or_active_policies() {
    let verifier =
        create_npm_resolution_verifier(default_opts("http://nonexistent.example.invalid/"));
    let name: PkgName = "acme".parse().expect("parse");
    let resolution = LockfileResolution::Registry(RegistryResolution {
        integrity: fake_integrity(),
        revision: None,
    });
    let ctx = pnpm_resolving_resolver_base::VerifyCtx {
        name: &name,
        version: "1.0.0",
        registry_name: Some("undeclared"),
    };
    let result = verifier.verify(&resolution, ctx).await;
    assert!(
        matches!(result, ResolutionVerification::Err { code: "MISSING_NAMED_REGISTRY", .. }),
        "got {result:?}",
    );
}

#[tokio::test]
async fn published_tarball_mapper_binds_routed_pnpr_install_v2_documents() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let packument = serde_json::json!({
        "name": "@corp/v2",
        "distTags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "integrity": FAKE_INTEGRITY,
                "manifest": {},
            }
        }
    });
    let _meta_mock = server
        .mock("GET", "/@corp%2Fv2")
        .with_header("content-type", "application/vnd.pnpr.install-v2+json")
        .with_body(packument.to_string())
        .expect_at_least(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    let mut reg_opts = std::collections::BTreeMap::new();
    reg_opts.insert(
        registry.clone(),
        pnpm_lockfile::RegistryOptions {
            server_type: Some(pnpm_lockfile::RegistryServerType::Pnpr),
            supports_time_field: None,
        },
    );
    opts.registry_options_by_url = Some(reg_opts);
    opts.artifacts.published_tarball_mapper =
        Some(std::sync::Arc::new(|_, pkg, ver, lockfile_url, _| {
            let routed = format!("https://pnpr.example/npm/~corp/{pkg}@{ver}");
            crate::MappedTarballUrl {
                accepted_routed_url: lockfile_url
                    .starts_with("https://pnpr.example/npm/~")
                    .then(|| routed.clone()),
                display_url: routed,
                accept_published_url: false,
            }
        }));
    let verifier = create_npm_resolution_verifier(opts);
    let name: PkgName = "@corp/v2".parse().expect("parse");
    let ok =
        tarball_resolution("https://pnpr.example/npm/~corp/@corp/v2@1.0.0", Some(fake_integrity()));
    assert_eq!(verifier.verify(&ok, ctx(&name, "1.0.0")).await, ResolutionVerification::Ok);
    let unrouted =
        tarball_resolution(&format!("{registry}@corp/v2/-/v2-1.0.0.tgz"), Some(fake_integrity()));
    assert!(matches!(
        verifier.verify(&unrouted, ctx(&name, "1.0.0")).await,
        ResolutionVerification::Err { code: "TARBALL_URL_MISMATCH", .. },
    ));
}

#[tokio::test]
async fn published_tarball_mapper_preserves_hosted_pnpr_install_v2_revision_urls() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let sha512 = ssri::IntegrityOpts::new()
        .algorithm(ssri::Algorithm::Sha512)
        .chain(b"rev3")
        .result();
    let packument = serde_json::json!({
        "name": "@corp/v2",
        "distTags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "integrity": sha512.to_string(),
                "revision": 3,
                "manifest": {},
            }
        }
    });
    let _meta_mock = server
        .mock("GET", "/@corp%2Fv2")
        .with_header("content-type", "application/vnd.pnpr.install-v2+json")
        .with_body(packument.to_string())
        .expect_at_least(1)
        .create_async()
        .await;
    let mut opts = default_opts(&registry);
    let mut reg_opts = std::collections::BTreeMap::new();
    reg_opts.insert(
        registry.clone(),
        pnpm_lockfile::RegistryOptions {
            server_type: Some(pnpm_lockfile::RegistryServerType::Pnpr),
            supports_time_field: None,
        },
    );
    opts.registry_options_by_url = Some(reg_opts);
    opts.artifacts.published_tarball_mapper =
        Some(std::sync::Arc::new(|_, _, _, _, published| crate::MappedTarballUrl {
            accepted_routed_url: Some(published.to_string()),
            display_url: published.to_string(),
            accept_published_url: false,
        }));
    let verifier = create_npm_resolution_verifier(opts);
    let name: PkgName = "@corp/v2".parse().expect("parse");
    let rev_url = format!("{registry}@corp/v2@1.0.0+r3");
    let ok = LockfileResolution::Tarball(pnpm_lockfile::TarballResolution {
        tarball: rev_url,
        integrity: Some(sha512),
        revision: Some(pnpm_lockfile::TarballRevision::try_from(3).expect("rev")),
        git_hosted: None,
        path: None,
    });
    assert_eq!(verifier.verify(&ok, ctx(&name, "1.0.0")).await, ResolutionVerification::Ok);
}

#[tokio::test]
async fn unmapped_tarball_mismatch_strips_presigned_query_and_fragment() {
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let packument = serde_json::json!({
        "name": "public-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "public-pkg",
                "version": "1.0.0",
                "dist": {
                    "integrity": FAKE_INTEGRITY,
                    "tarball": "https://cdn.public.example/public-pkg-1.0.0.tgz?X-Amz-Signature=UPSTREAM_PRESIGNED_SECRET",
                }
            }
        }
    });
    let _meta_mock = server
        .mock("GET", "/public-pkg")
        .with_body(packument.to_string())
        .create_async()
        .await;
    let verifier = create_npm_resolution_verifier(default_opts(&registry));
    let name: PkgName = "public-pkg".parse().expect("parse");
    let bad = tarball_resolution(
        "https://other.public.example/public-pkg-1.0.0.tgz?X-Amz-Signature=LOCKFILE_PRESIGNED_SECRET#frag",
        Some(fake_integrity()),
    );
    let ResolutionVerification::Err { code, reason } =
        verifier.verify(&bad, ctx(&name, "1.0.0")).await
    else {
        panic!("expected mismatch");
    };
    assert_eq!(code, "TARBALL_URL_MISMATCH");
    for secret in ["UPSTREAM_PRESIGNED_SECRET", "LOCKFILE_PRESIGNED_SECRET", "frag"] {
        assert!(!reason.contains(secret), "{secret} leaked in {reason}");
    }
}
