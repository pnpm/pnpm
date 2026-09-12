use super::{
    ARTIFACTORY_REGISTRY, BTreeMap, GIT_COMMIT, LockfileResolution, RegistryOptions,
    RegistryServerType, TarballResolution, assert_eq, libc_matches, registry_server_type,
    select_platform_variant, selector, target, variant,
};

/// The flag is a hint: the fetch dispatch and the store-index key
/// follow the URL, so a git-host archive URL counts as git-hosted even
/// when the lockfile says otherwise. A lockfile claiming `false` on
/// one would otherwise skip the prepare + packlist pass and install the
/// raw archive.
#[test]
fn is_git_hosted_follows_the_url_over_a_contradicting_flag() {
    let git_hosted_url = format!("https://codeload.github.com/foo/bar/tar.gz/{GIT_COMMIT}");

    for flag in [None, Some(false), Some(true)] {
        let resolution = TarballResolution {
            tarball: git_hosted_url.clone(),
            integrity: None,
            revision: None,
            git_hosted: flag,
            path: None,
        };
        assert!(resolution.is_git_hosted(), "a git-host archive URL is git-hosted, {flag:?}");
    }

    let plain = TarballResolution {
        tarball: "https://example.com/pkg-1.0.0.tgz".to_string(),
        integrity: None,
        revision: None,
        git_hosted: None,
        path: None,
    };
    assert!(!plain.is_git_hosted());
}

#[test]
fn pick_first_matching_variant() {
    let variants = vec![
        variant("darwin-arm64", vec![target("darwin", "arm64", None)]),
        variant("linux-x64", vec![target("linux", "x64", None)]),
    ];
    let picked = select_platform_variant(&variants, &selector("linux", "x64", Some("glibc")))
        .expect("matching variant");
    assert_eq!(
        picked.resolution.integrity().map(ToString::to_string),
        Some("sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==".to_string()),
        "picked variant should be the linux-x64 one (url is opaque to integrity, but the structural fixture means both share the same hash)",
    );
    assert_eq!(picked.targets, vec![target("linux", "x64", None)]);
}

#[test]
fn pick_matches_any_target_in_a_variant() {
    let variants = vec![variant(
        "darwin-universal",
        vec![target("darwin", "arm64", None), target("darwin", "x64", None)],
    )];
    let picked = select_platform_variant(&variants, &selector("darwin", "x64", None));
    assert!(picked.is_some());
}

#[test]
fn pick_returns_none_when_no_variant_matches() {
    let variants = vec![variant("darwin-arm64", vec![target("darwin", "arm64", None)])];
    assert!(select_platform_variant(&variants, &selector("linux", "x64", Some("glibc"))).is_none());
}

#[test]
fn pick_rejects_default_variant_for_musl_host() {
    let variants = vec![variant("linux-x64-glibc", vec![target("linux", "x64", None)])];
    assert!(
        select_platform_variant(&variants, &selector("linux", "x64", Some("musl"))).is_none(),
        "musl host must not silently pick the glibc default variant",
    );
}

#[test]
fn pick_returns_first_when_multiple_variants_match() {
    let variants = vec![
        variant("first-darwin-arm64", vec![target("darwin", "arm64", None)]),
        variant("second-darwin-arm64", vec![target("darwin", "arm64", None)]),
    ];
    let picked = select_platform_variant(&variants, &selector("darwin", "arm64", None))
        .expect("matching variant");
    let LockfileResolution::Binary(inner) = &picked.resolution else {
        panic!("expected Binary inner resolution");
    };
    assert_eq!(inner.url, "first-darwin-arm64", "declaration order must win");
}

#[test]
fn pick_matches_musl_variant_for_musl_host() {
    let variants = vec![
        variant("linux-x64-glibc", vec![target("linux", "x64", None)]),
        variant("linux-x64-musl", vec![target("linux", "x64", Some("musl"))]),
    ];
    let picked = select_platform_variant(&variants, &selector("linux", "x64", Some("musl")))
        .expect("musl variant present");
    assert_eq!(picked.targets, vec![target("linux", "x64", Some("musl"))]);
}

#[test]
fn libc_matches_truth_table() {
    assert!(libc_matches(None, None));
    assert!(!libc_matches(Some("musl"), None));
    assert!(!libc_matches(Some("glibc"), None));

    assert!(libc_matches(None, Some("glibc")));
    assert!(!libc_matches(Some("musl"), Some("glibc")));

    assert!(libc_matches(Some("musl"), Some("musl")));
    assert!(!libc_matches(None, Some("musl")));

    assert!(libc_matches(Some("uclibc"), Some("uclibc")));
    assert!(!libc_matches(None, Some("uclibc")));
    assert!(!libc_matches(Some("glibc"), Some("uclibc")));
}

#[test]
fn registry_server_type_is_undeclared_by_default_and_tolerates_a_missing_trailing_slash() {
    let options = BTreeMap::from([(
        ARTIFACTORY_REGISTRY.to_string(),
        RegistryOptions {
            server_type: Some(RegistryServerType::Artifactory),
            supports_time_field: None,
        },
    )]);
    assert_eq!(
        registry_server_type(&options, ARTIFACTORY_REGISTRY.trim_end_matches('/')),
        Some(RegistryServerType::Artifactory),
    );
    assert_eq!(registry_server_type(&options, "https://npm.example.com/"), None);
    assert_eq!(registry_server_type(&BTreeMap::new(), ARTIFACTORY_REGISTRY), None);
}
