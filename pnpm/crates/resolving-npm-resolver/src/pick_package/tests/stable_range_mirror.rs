use super::{
    ABBREVIATED_META_DIR, AuthHeaders, DateTime, EXISTING_VERSION_SELECTOR_WEIGHT,
    InMemoryPackageMetaCache, PACKAGE_BODY, TempDir, ThrottledClient, Utc, VersionSelectorEntry,
    VersionSelectorType, VersionSelectorWithWeight, VersionSelectors, assert_eq,
    create_package_version_policy, default_opts, get_pkg_mirror_path, parse_cutoff,
    persist_meta_to_mirror, pick_package, range_spec, shared_packument_fetch_locker,
    version_selection::{public_ctx, set_mirror_mtime},
};
use pnpm_config::version_policy::PackageVersionPolicy;

/// Between the fixture's 1.0.0 and 1.1.0 publish times.
const CUTOFF: &str = "2024-06-01T00:00:00Z";

/// The mirror a pick finds, and the release-age policy it picks under. The
/// fixture's mirror carries no `ETag`; a `fresh` one is written just now, so
/// the shortcut for a young mirror without an `ETag` may answer from it. With
/// `dropped_after_first_pick`, only the packument the first pick left in
/// memory can answer the second.
struct Mirror<'a> {
    fresh: bool,
    dropped_after_first_pick: bool,
    published_by: Option<DateTime<Utc>>,
    published_by_exclude: Option<&'a PackageVersionPolicy>,
}

impl Mirror<'_> {
    const PLAIN: Mirror<'static> = Mirror {
        fresh: false,
        dropped_after_first_pick: false,
        published_by: None,
        published_by_exclude: None,
    };
}

/// Pick `^1.0.0` for `acme` twice against a mirror warmed with
/// [`PACKAGE_BODY`] and return the version picked. The registry must be
/// asked exactly `expected_requests` times across both picks. A mirror that
/// is not `fresh` is dated in the past, so neither the young-mirror nor the
/// `minimumReleaseAge` freshness shortcut can answer the pick.
async fn pick_from_warm_mirror(
    selectors: &VersionSelectors,
    mirror: Mirror<'_>,
    expected_requests: usize,
) -> String {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/acme")
        .with_status(200)
        .with_header("etag", r#"W/"fresh""#)
        .with_body(PACKAGE_BODY)
        .expect(expected_requests)
        .create_async()
        .await;
    let cache_dir = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let cached: pnpm_registry::Package =
        serde_json::from_str(PACKAGE_BODY).expect("parse packument");
    persist_meta_to_mirror(cache_dir.path(), ABBREVIATED_META_DIR, &registry, &cached)
        .expect("warm mirror");
    if !mirror.fresh {
        set_mirror_mtime(cache_dir.path(), &registry, parse_cutoff("2024-01-01T00:00:00Z").into());
    }
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let meta_cache = InMemoryPackageMetaCache::default();
    let fetch_locker = shared_packument_fetch_locker();
    let ctx = public_ctx(&cache_dir, &http_client, &auth_headers, &meta_cache, &fetch_locker);
    let mut opts = default_opts(&registry);
    opts.preferred_version_selectors = Some(selectors);
    opts.policy.published_by = mirror.published_by;
    opts.policy.published_by_exclude = mirror.published_by_exclude;

    let first = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("first");
    if mirror.dropped_after_first_pick {
        let path = get_pkg_mirror_path(cache_dir.path(), ABBREVIATED_META_DIR, &registry, "acme")
            .expect("mirror path");
        std::fs::remove_file(path).expect("drop mirror");
    }
    let second = pick_package(&ctx, &range_spec("acme", "^1.0.0"), &opts).await.expect("second");
    let first = first.picked_package.expect("first pick").version.to_string();
    assert_eq!(second.picked_package.expect("second pick").version.to_string(), first);
    mock.assert_async().await;
    first
}

fn lockfile_selectors(versions: &[&str]) -> VersionSelectors {
    versions
        .iter()
        .map(|version| {
            (
                (*version).to_string(),
                VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
                    selector_type: VersionSelectorType::Version,
                    weight: EXISTING_VERSION_SELECTOR_WEIGHT,
                }),
            )
        })
        .collect()
}

#[tokio::test]
async fn release_age_version_scoped_exclude_revalidates_the_mirror() {
    let policy = create_package_version_policy(["acme@1.0.0"]).expect("policy");
    let mirror = Mirror {
        published_by: Some(parse_cutoff(CUTOFF)),
        published_by_exclude: Some(&policy),
        ..Mirror::PLAIN
    };
    let picked = pick_from_warm_mirror(&lockfile_selectors(&["1.0.0"]), mirror, 1).await;
    assert_eq!(picked, "1.0.0");
}

#[tokio::test]
async fn release_age_excluded_range_reuses_a_fresh_mirror_without_an_etag() {
    let policy = create_package_version_policy(["acme"]).expect("policy");
    let mirror = Mirror {
        fresh: true,
        dropped_after_first_pick: false,
        published_by: Some(parse_cutoff("2100-01-01T00:00:00Z")),
        published_by_exclude: Some(&policy),
    };
    let picked = pick_from_warm_mirror(&VersionSelectors::new(), mirror, 0).await;
    assert_eq!(picked, "1.1.0");
}

#[tokio::test]
async fn release_age_range_revalidates_a_fresh_mirror_without_an_etag() {
    let mirror = Mirror {
        fresh: true,
        dropped_after_first_pick: false,
        published_by: Some(parse_cutoff("2100-01-01T00:00:00Z")),
        published_by_exclude: None,
    };
    let picked = pick_from_warm_mirror(&VersionSelectors::new(), mirror, 1).await;
    assert_eq!(picked, "1.1.0");
}

#[tokio::test]
async fn release_age_excluded_range_reuses_the_packument_left_in_memory() {
    let policy = create_package_version_policy(["acme"]).expect("policy");
    let mirror = Mirror {
        dropped_after_first_pick: true,
        published_by: Some(parse_cutoff(CUTOFF)),
        published_by_exclude: Some(&policy),
        ..Mirror::PLAIN
    };
    let picked = pick_from_warm_mirror(&lockfile_selectors(&["1.1.0"]), mirror, 0).await;
    assert_eq!(picked, "1.1.0");
}
