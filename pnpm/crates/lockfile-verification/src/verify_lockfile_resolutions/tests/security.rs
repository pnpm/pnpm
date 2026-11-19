use super::{
    Arc, CapturingVerifier, FetchFails, Mutex, RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE,
    ResolutionVerifier, SilentReporter, TempDir, VerifyError, VerifyLockfileResolutionsOptions,
    collect_resolution_policy_violations, lockfile_verification_is_cached,
    lockfile_verification_is_cached_by_content, parse, verify_lockfile_resolutions,
    with_offline_check_cache_identities,
};

#[tokio::test]
async fn rejects_a_traversal_snapshot_package_name() {
    // The snapshot's own package name (the `snapshots:` key) becomes
    // `<slot>/node_modules/<name>` at extract time — a sink the alias
    // scan of the dependency maps alone doesn't cover.
    let lockfile = parse(
        "lockfileVersion: '9.0'

packages:

  ../../../escape@1.0.0:
    resolution: {integrity: sha512-deadbeef}

snapshots:

  ../../../escape@1.0.0: {}
",
    );
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect_err("a traversal snapshot package name must be rejected");
    let VerifyError::InvalidDependencyAlias { breakdown, .. } = err else {
        panic!("expected InvalidDependencyAlias, got {err:?}");
    };
    assert!(breakdown.contains("../../../escape"), "breakdown {breakdown:?}");
}

#[test]
fn rejects_a_traversal_package_key_when_snapshots_absent() {
    let lockfile = parse(
        "lockfileVersion: '9.0'

packages:

  ../../../escape@1.0.0:
    resolution: {integrity: sha512-deadbeef}
",
    );
    let err = crate::verify_lockfile_dependency_names(&lockfile)
        .expect_err("a traversal packages key must be rejected even without snapshots");
    let VerifyError::InvalidDependencyAlias { breakdown, .. } = err else {
        panic!("expected InvalidDependencyAlias, got {err:?}");
    };
    assert!(breakdown.contains("../../../escape"), "breakdown {breakdown:?}");
}

async fn assert_shape_violation_for_resolution(label: &str, resolution: &str) {
    let yaml = format!(
        "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      safe-pkg:
        specifier: 1.0.0
        version: 1.0.0
packages:
  safe-pkg@1.0.0:
    resolution: {resolution}
snapshots:
  safe-pkg@1.0.0: {{}}
",
    );
    let lockfile = parse(&yaml);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let capturing: Arc<dyn ResolutionVerifier> =
        Arc::new(CapturingVerifier { seen: Arc::clone(&seen), policy: serde_json::Map::new() });
    let fetch_fails =
        FetchFails::new("upstream must not be contacted") as Arc<dyn ResolutionVerifier>;

    for verifiers in [&[][..], &[capturing][..], &[fetch_fails][..]] {
        let violations = collect_resolution_policy_violations(&lockfile, verifiers, None)
            .await
            .expect("shape mismatch returns violations instead of transport error");
        assert_eq!(violations.len(), 1, "{label}: {violations:?}");
        assert_eq!(violations[0].name.to_string(), "safe-pkg", "{label}");
        assert_eq!(violations[0].version, "1.0.0", "{label}");
        assert_eq!(violations[0].code, RESOLUTION_SHAPE_MISMATCH_VIOLATION_CODE, "{label}");
    }
    assert!(
        seen.lock()
            .expect("seen lock")
            .is_empty(),
        "{label} must short-circuit before running registry verifiers",
    );
}

#[tokio::test]
async fn collect_resolution_policy_violations_reports_shape_mismatches() {
    let cases = [
        ("file-tarball", "{integrity: sha512-deadbeef, tarball: 'file:alt.tgz'}"),
        ("directory", "{type: directory, directory: '../alt'}"),
        (
            "git",
            "{type: git, repo: 'https://example.com/safe-pkg.git', commit: 0123456789abcdef0123456789abcdef01234567}",
        ),
    ];
    for (label, resolution) in cases {
        assert_shape_violation_for_resolution(label, resolution).await;
    }
}

#[tokio::test]
async fn poisoned_cache_record_cannot_bypass_resolution_shape_mismatch() {
    let yaml = "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      safe-pkg:
        specifier: 1.0.0
        version: 1.0.0
packages:
  safe-pkg@1.0.0:
    resolution: {integrity: sha512-deadbeef, tarball: 'file:alt.tgz'}
snapshots:
  safe-pkg@1.0.0: {}
";
    let dir = TempDir::new().expect("tempdir");
    let lockfile_path = dir.path().join("pnpm-lock.yaml");
    std::fs::write(&lockfile_path, yaml).expect("write lockfile");
    let lockfile = parse(yaml);
    let cache_dir = dir.path().join("cache");
    let verifiers = with_offline_check_cache_identities(&[]);

    // Simulate a pre-existing poisoned cache entry written by an unpatched build.
    crate::record_verification(
        &cache_dir,
        &lockfile_path,
        &verifiers,
        || crate::hash_lockfile(&lockfile),
        crate::CachePrecomputed::default(),
    );
    assert!(cache_dir.join(crate::CACHE_FILE_NAME).exists());

    assert!(!lockfile_verification_is_cached(&cache_dir, &lockfile_path, &lockfile, &[]));
    assert!(!lockfile_verification_is_cached_by_content(&cache_dir, &lockfile, &[]));

    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            lockfile_path: Some(&lockfile_path),
            cache_dir: Some(&cache_dir),
            ..Default::default()
        },
    )
    .await
    .expect_err("poisoned cache entry must not bypass resolution shape check");
    assert!(matches!(err, VerifyError::ResolutionShapeMismatch { .. }), "got {err:?}");
}
