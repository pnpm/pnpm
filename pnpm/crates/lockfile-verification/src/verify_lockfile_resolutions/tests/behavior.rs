use super::{
    AlwaysFail, Arc, AtomicUsize, FailFor, FetchFails, LockfileResolution, LogEvent, Mutex,
    Ordering, PkgName, Reporter, ResolutionVerification, ResolutionVerifier, SINGLE_PKG_LOCKFILE,
    SilentReporter, TWO_PKG_LOCKFILE, TempDir, VerifyCtx, VerifyError, VerifyFuture,
    VerifyLockfileResolutionsOptions, collect_resolution_policy_violations, parse,
    verify_lockfile_resolutions,
};

#[tokio::test]
async fn a_fetch_failure_aborts_with_the_registry_error() {
    let lockfile = parse(SINGLE_PKG_LOCKFILE);
    let verifier =
        FetchFails::new("Failed to fetch metadata from https://registry.example/acme: 403");
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[verifier as Arc<dyn ResolutionVerifier>],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect_err("a transport failure must abort verification");
    // It surfaces the registry's own error, not a lockfile-policy batch.
    let VerifyError::RegistryMetaFetchFailed { message } = err else {
        panic!("expected RegistryMetaFetchFailed, got: {err:?}");
    };
    assert!(message.contains("403"), "message: {message}");
}

#[tokio::test]
async fn no_verifiers_is_a_noop() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();
    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let lockfile = parse(SINGLE_PKG_LOCKFILE);
    let result = verify_lockfile_resolutions::<RecordingReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await;
    assert!(result.is_ok());
    assert!(EVENTS.lock().unwrap().is_empty(), "no-op must not emit");
}

#[tokio::test]
async fn no_packages_section_is_a_noop() {
    let yaml = "lockfileVersion: '9.0'\n\nimporters:\n\n  .: {}\n";
    let lockfile = parse(yaml);
    let verifier = AlwaysFail::new("WHATEVER", "should never run");
    let result = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[verifier as Arc<dyn ResolutionVerifier>],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn single_violation_picks_per_policy_variant() {
    let lockfile = parse(SINGLE_PKG_LOCKFILE);
    let verifier = AlwaysFail::new("MINIMUM_RELEASE_AGE_VIOLATION", "was published yesterday");
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[verifier as Arc<dyn ResolutionVerifier>],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect_err("violation must surface as Err");
    let VerifyError::MinimumReleaseAgeViolation { count, breakdown } = err else {
        panic!("expected MinimumReleaseAgeViolation, got: {err:?}");
    };
    assert_eq!(count, 1);
    assert!(breakdown.contains("react@17.0.2"), "got: {breakdown}");
}

#[tokio::test]
async fn mixed_code_batch_escalates() {
    let lockfile = parse(TWO_PKG_LOCKFILE);
    let min_age = FailFor::new("MINIMUM_RELEASE_AGE_VIOLATION", "young", vec!["acme"]);
    let trust = FailFor::new("TRUST_DOWNGRADE", "downgrade", vec!["bravo"]);
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[min_age as Arc<dyn ResolutionVerifier>, trust as Arc<dyn ResolutionVerifier>],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect_err("mixed batch must surface as Err");
    let VerifyError::LockfileResolutionVerification { count, breakdown } = err else {
        panic!("expected LockfileResolutionVerification, got: {err:?}");
    };
    assert_eq!(count, 2);
    assert!(breakdown.contains("[MINIMUM_RELEASE_AGE_VIOLATION]"));
    assert!(breakdown.contains("[TRUST_DOWNGRADE]"));
    // Sorted by name@version: acme before bravo.
    let acme = breakdown.find("acme").expect("acme present");
    let bravo = breakdown.find("bravo").expect("bravo present");
    assert!(acme < bravo, "expected acme before bravo: {breakdown}");
}

#[tokio::test]
async fn per_candidate_fan_out_stops_at_first_failure() {
    let lockfile = parse(SINGLE_PKG_LOCKFILE);
    let first = AlwaysFail::new("MINIMUM_RELEASE_AGE_VIOLATION", "first");
    let second = AlwaysFail::new("TRUST_DOWNGRADE", "second");
    let violations = collect_resolution_policy_violations(
        &lockfile,
        &[first as Arc<dyn ResolutionVerifier>, second as Arc<dyn ResolutionVerifier>],
        None,
    )
    .await
    .expect("no fetch failure");
    assert_eq!(violations.len(), 1, "stop at first failing verifier");
    assert_eq!(violations[0].code, "MINIMUM_RELEASE_AGE_VIOLATION");
}

#[tokio::test]
async fn collect_returns_data_for_all_violations() {
    let lockfile = parse(TWO_PKG_LOCKFILE);
    let verifier = AlwaysFail::new("MINIMUM_RELEASE_AGE_VIOLATION", "young");
    let violations = collect_resolution_policy_violations(
        &lockfile,
        &[verifier as Arc<dyn ResolutionVerifier>],
        None,
    )
    .await
    .expect("no fetch failure");
    assert_eq!(violations.len(), 2);
}

/// Pacquet's `Lockfile` keeps `packages:` keyed by the bare
/// `react@17.0.2` already, so the duplicate would have to live in
/// `snapshots:` — which today's collector doesn't walk.
#[tokio::test]
async fn one_packages_entry_yields_one_verification() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CALLS: AtomicUsize = AtomicUsize::new(0);
    CALLS.store(0, Ordering::SeqCst);

    struct Counting {
        policy: serde_json::Map<String, serde_json::Value>,
    }
    impl ResolutionVerifier for Counting {
        fn verify<'a>(
            &'a self,
            _resolution: &'a LockfileResolution,
            _ctx: VerifyCtx<'a>,
        ) -> VerifyFuture<'a> {
            CALLS.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { ResolutionVerification::Ok })
        }

        fn policy(&self) -> &serde_json::Map<String, serde_json::Value> {
            &self.policy
        }

        fn can_trust_past_check(
            &self,
            _cached: &serde_json::Map<String, serde_json::Value>,
        ) -> bool {
            true
        }
    }

    let lockfile = parse(SINGLE_PKG_LOCKFILE);
    let verifier: Arc<dyn ResolutionVerifier> =
        Arc::new(Counting { policy: serde_json::Map::new() });
    verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[verifier],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect("all-ok");
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn rejected_verification_does_not_write_a_cache_record() {
    let dir = TempDir::new().expect("tempdir");
    let lockfile_path = dir.path().join("pnpm-lock.yaml");
    std::fs::write(&lockfile_path, SINGLE_PKG_LOCKFILE).expect("write lockfile");
    let lockfile = parse(SINGLE_PKG_LOCKFILE);
    let cache_dir = dir.path().join("cache");
    let verifier = AlwaysFail::new("MINIMUM_RELEASE_AGE_VIOLATION", "too new");

    verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[verifier as Arc<dyn ResolutionVerifier>],
        &VerifyLockfileResolutionsOptions {
            lockfile_path: Some(&lockfile_path),
            cache_dir: Some(&cache_dir),
            ..Default::default()
        },
    )
    .await
    .expect_err("verification must reject");

    assert!(!cache_dir.join(crate::CACHE_FILE_NAME).exists());
}

#[tokio::test]
async fn uninterested_verifier_skips_candidate_fan_out() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    CALLS.store(0, Ordering::SeqCst);

    struct Uninterested {
        policy: serde_json::Map<String, serde_json::Value>,
    }
    impl ResolutionVerifier for Uninterested {
        fn might_verify(&self, _resolution: &LockfileResolution, _ctx: VerifyCtx<'_>) -> bool {
            false
        }

        fn verify<'a>(
            &'a self,
            _resolution: &'a LockfileResolution,
            _ctx: VerifyCtx<'a>,
        ) -> VerifyFuture<'a> {
            CALLS.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { ResolutionVerification::Ok })
        }

        fn policy(&self) -> &serde_json::Map<String, serde_json::Value> {
            &self.policy
        }

        fn can_trust_past_check(
            &self,
            _cached: &serde_json::Map<String, serde_json::Value>,
        ) -> bool {
            true
        }
    }

    let lockfile = parse(SINGLE_PKG_LOCKFILE);
    let verifier: Arc<dyn ResolutionVerifier> =
        Arc::new(Uninterested { policy: serde_json::Map::new() });
    verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[verifier],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect("all-ok");
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn second_run_with_cache_skips_fan_out() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    CALLS.store(0, Ordering::SeqCst);

    struct Counting {
        policy: serde_json::Map<String, serde_json::Value>,
    }
    impl ResolutionVerifier for Counting {
        fn verify<'a>(
            &'a self,
            _resolution: &'a LockfileResolution,
            _ctx: VerifyCtx<'a>,
        ) -> VerifyFuture<'a> {
            CALLS.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { ResolutionVerification::Ok })
        }
        fn policy(&self) -> &serde_json::Map<String, serde_json::Value> {
            &self.policy
        }
        fn can_trust_past_check(
            &self,
            _cached: &serde_json::Map<String, serde_json::Value>,
        ) -> bool {
            true
        }
    }

    let dir = TempDir::new().expect("tempdir");
    let lockfile_path = dir.path().join("pnpm-lock.yaml");
    std::fs::write(&lockfile_path, SINGLE_PKG_LOCKFILE).expect("write lockfile");
    let lockfile = parse(SINGLE_PKG_LOCKFILE);
    let cache_dir = dir.path().join("cache");
    let verifier: Arc<dyn ResolutionVerifier> =
        Arc::new(Counting { policy: serde_json::Map::new() });
    let opts = VerifyLockfileResolutionsOptions {
        lockfile_path: Some(&lockfile_path),
        cache_dir: Some(&cache_dir),
        ..Default::default()
    };

    verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        std::slice::from_ref(&verifier),
        &opts,
    )
    .await
    .expect("first run");
    assert_eq!(CALLS.load(Ordering::SeqCst), 1, "first run ran the verifier");

    verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        std::slice::from_ref(&verifier),
        &opts,
    )
    .await
    .expect("second run");
    assert_eq!(CALLS.load(Ordering::SeqCst), 1, "second run skipped via cache");
}

/// The shape-only run that every install performs must not announce
/// supply-chain policies the user never configured.
#[tokio::test]
async fn cache_hit_with_no_policy_verifiers_stays_silent() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();
    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let dir = TempDir::new().expect("tempdir");
    let lockfile_path = dir.path().join("pnpm-lock.yaml");
    std::fs::write(&lockfile_path, SINGLE_PKG_LOCKFILE).expect("write lockfile");
    let lockfile = parse(SINGLE_PKG_LOCKFILE);
    let cache_dir = dir.path().join("cache");
    let verifier = FailFor::new("UNUSED", "n/a", vec![]) as Arc<dyn ResolutionVerifier>;
    let opts = VerifyLockfileResolutionsOptions {
        lockfile_path: Some(&lockfile_path),
        cache_dir: Some(&cache_dir),
        ..Default::default()
    };

    verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        std::slice::from_ref(&verifier),
        &opts,
    )
    .await
    .expect("first run");
    verify_lockfile_resolutions::<RecordingReporter>(&lockfile, &[], &opts)
        .await
        .expect("second run");

    let captured = EVENTS.lock().unwrap();
    assert!(captured.is_empty(), "shape-only cache hit must not emit, got: {captured:?}");
}

/// Catches a regression where `PkgName` would be passed into the
/// ctx as a borrow whose lifetime didn't outlive the future.
#[tokio::test]
async fn ctx_borrows_have_expected_lifetimes() {
    let lockfile = parse(SINGLE_PKG_LOCKFILE);
    let _: PkgName = "react".parse().expect("PkgName parses");
    let result = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn rejects_invalid_alias_nested_in_snapshot() {
    let lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      real:
        specifier: ^1.0.0
        version: 1.0.0

packages:

  real@1.0.0:
    resolution: {integrity: sha512-deadbeef}

snapshots:

  real@1.0.0:
    dependencies:
      '../../../escape': 1.0.0
",
    );
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect_err("invalid alias nested in a snapshot must be rejected");
    assert!(matches!(err, VerifyError::InvalidDependencyAlias { .. }), "got {err:?}");
}

#[tokio::test]
async fn accepts_valid_scoped_and_unscoped_aliases() {
    let lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.0.0
      '@scope/bar':
        specifier: ^1.0.0
        version: 1.0.0

packages:

  foo@1.0.0:
    resolution: {integrity: sha512-deadbeef}

  '@scope/bar@1.0.0':
    resolution: {integrity: sha512-deadbeef}

snapshots:

  foo@1.0.0: {}
  '@scope/bar@1.0.0': {}
",
    );
    verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect("valid scoped and unscoped aliases pass");
}
