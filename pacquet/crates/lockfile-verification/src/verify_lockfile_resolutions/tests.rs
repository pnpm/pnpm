use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use pacquet_lockfile::{Lockfile, LockfileResolution, PkgName};
use pacquet_reporter::{LockfileVerificationMessage, LogEvent, Reporter, SilentReporter};
use pacquet_resolving_resolver_base::{
    ResolutionVerification, ResolutionVerifier, VerifyCtx, VerifyFuture,
};
use tempfile::TempDir;

use super::{
    VerifyLockfileResolutionsOptions, collect_resolution_policy_violations,
    verify_lockfile_resolutions,
};
use crate::VerifyError;

const SINGLE_PKG_LOCKFILE: &str = "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      react:
        specifier: ^17.0.2
        version: 17.0.2

packages:

  react@17.0.2:
    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}

snapshots:

  react@17.0.2: {}
";

const TWO_PKG_LOCKFILE: &str = "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      acme:
        specifier: ^1.0.0
        version: 1.0.0
      bravo:
        specifier: ^2.0.0
        version: 2.0.0

packages:

  acme@1.0.0:
    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}

  bravo@2.0.0:
    resolution: {integrity: sha512-s4h96KtLDUQlsENhMn1ar8t2bEa+q/YAtj8pPPdIjPDGBDIVNsrD9aXNWqspUe6AzKCIG0C1HZZLqLV7qpOBGA==}

snapshots:

  acme@1.0.0: {}
  bravo@2.0.0: {}
";

fn parse(yaml: &str) -> Lockfile {
    serde_saphyr::from_str(yaml).expect("parse fixture lockfile")
}

/// Reject every candidate with the given code/reason.
struct AlwaysFail {
    code: &'static str,
    reason: &'static str,
    policy: serde_json::Map<String, serde_json::Value>,
}

impl AlwaysFail {
    fn new(code: &'static str, reason: &'static str) -> Arc<Self> {
        Arc::new(Self { code, reason, policy: serde_json::Map::new() })
    }
}

impl ResolutionVerifier for AlwaysFail {
    fn verify<'a>(
        &'a self,
        _resolution: &'a LockfileResolution,
        _ctx: VerifyCtx<'a>,
    ) -> VerifyFuture<'a> {
        let code = self.code;
        let reason = self.reason.to_string();
        Box::pin(async move { ResolutionVerification::Err { code, reason } })
    }

    fn policy(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.policy
    }

    fn can_trust_past_check(&self, _cached: &serde_json::Map<String, serde_json::Value>) -> bool {
        true
    }
}

/// Reject only specific package names; pass everything else.
struct FailFor {
    code: &'static str,
    reason: &'static str,
    names: Vec<&'static str>,
    policy: serde_json::Map<String, serde_json::Value>,
}

impl FailFor {
    fn new(code: &'static str, reason: &'static str, names: Vec<&'static str>) -> Arc<Self> {
        Arc::new(Self { code, reason, names, policy: serde_json::Map::new() })
    }
}

impl ResolutionVerifier for FailFor {
    fn verify<'a>(
        &'a self,
        _resolution: &'a LockfileResolution,
        ctx: VerifyCtx<'a>,
    ) -> VerifyFuture<'a> {
        let name = ctx.name.to_string();
        let triggers = self.names.contains(&name.as_str());
        let code = self.code;
        let reason = self.reason.to_string();
        Box::pin(async move {
            if triggers {
                ResolutionVerification::Err { code, reason }
            } else {
                ResolutionVerification::Ok
            }
        })
    }

    fn policy(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.policy
    }

    fn can_trust_past_check(&self, _cached: &serde_json::Map<String, serde_json::Value>) -> bool {
        true
    }
}

/// Empty verifier list is a no-op — neither emits nor errors. Lets
/// the install path skip the gate when no policy is configured.
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

/// Lockfile without `packages:` is a no-op — there's nothing to
/// verify. Mirrors upstream's `!lockfile.packages` guard.
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

/// All verifiers pass → success path returns `Ok`, fires
/// `Started` + `Done`, no `Failed`.
#[tokio::test]
async fn all_ok_emits_started_then_done() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();
    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let lockfile = parse(SINGLE_PKG_LOCKFILE);
    // Verifier that always returns Ok.
    let verifier = FailFor::new("UNUSED", "n/a", vec![]);
    let lockfile_path = Path::new("/p/lock.yaml");
    let opts = VerifyLockfileResolutionsOptions {
        lockfile_path: Some(lockfile_path),
        ..Default::default()
    };
    verify_lockfile_resolutions::<RecordingReporter>(
        &lockfile,
        &[verifier as Arc<dyn ResolutionVerifier>],
        &opts,
    )
    .await
    .expect("all-ok must succeed");

    let captured = EVENTS.lock().unwrap();
    assert_eq!(captured.len(), 2, "expected Started + Done, got: {captured:?}");
    match &captured[0] {
        LogEvent::LockfileVerification(log) => match &log.message {
            LockfileVerificationMessage::Started { entries, lockfile_path } => {
                assert_eq!(*entries, 1);
                assert_eq!(lockfile_path.as_deref(), Some("/p/lock.yaml"));
            }
            other => panic!("expected Started, got {other:?}"),
        },
        other => panic!("expected LockfileVerification, got {other:?}"),
    }
    match &captured[1] {
        LogEvent::LockfileVerification(log) => match &log.message {
            LockfileVerificationMessage::Done { entries, .. } => assert_eq!(*entries, 1),
            other => panic!("expected Done, got {other:?}"),
        },
        other => panic!("expected LockfileVerification, got {other:?}"),
    }
}

/// Single `MIN_AGE` violation → resolves to the per-policy variant
/// with that one entry in the breakdown.
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

/// Two verifiers with different codes both rejecting → mixed batch
/// escalates to `LockfileResolutionVerification`.
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

/// Per-candidate fan-out stops at the first verifier that rejects —
/// a single (name, version) never produces two violations even when
/// multiple verifiers would have flagged it.
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
    .await;
    assert_eq!(violations.len(), 1, "stop at first failing verifier");
    assert_eq!(violations[0].code, "MINIMUM_RELEASE_AGE_VIOLATION");
}

/// `collect_resolution_policy_violations` returns the data without
/// short-circuiting on the first batch. Used by auto-collect and
/// strict-mode prompt callers.
#[tokio::test]
async fn collect_returns_data_for_all_violations() {
    let lockfile = parse(TWO_PKG_LOCKFILE);
    let verifier = AlwaysFail::new("MINIMUM_RELEASE_AGE_VIOLATION", "young");
    let violations = collect_resolution_policy_violations(
        &lockfile,
        &[verifier as Arc<dyn ResolutionVerifier>],
        None,
    )
    .await;
    assert_eq!(violations.len(), 2);
}

/// Failed-path emit fires — the `Failed` variant pairs with the
/// `Started` even when the runner returns Err.
#[tokio::test]
async fn failed_path_emits_failed_terminator() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();
    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let lockfile = parse(SINGLE_PKG_LOCKFILE);
    let verifier = AlwaysFail::new("MINIMUM_RELEASE_AGE_VIOLATION", "young");
    let _ = verify_lockfile_resolutions::<RecordingReporter>(
        &lockfile,
        &[verifier as Arc<dyn ResolutionVerifier>],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await;

    let captured = EVENTS.lock().unwrap();
    assert_eq!(captured.len(), 2, "expected Started + Failed, got: {captured:?}");
    match &captured[1] {
        LogEvent::LockfileVerification(log) => assert!(
            matches!(log.message, LockfileVerificationMessage::Failed { .. }),
            "expected Failed, got: {:?}",
            log.message,
        ),
        other => panic!("expected LockfileVerification, got {other:?}"),
    }
}

/// The deduplicating-by-(name, version, resolution) candidate
/// collector means a snapshot key like `react@17.0.2(react@17.0.2)`
/// alongside the bare `react@17.0.2` would still only verify once.
/// Pacquet's `Lockfile` keeps `packages:` keyed by the bare
/// `react@17.0.2` already, so the duplicate would have to live in
/// `snapshots:` — which today's collector doesn't walk. This guards
/// the contract that a single packages-section entry produces a
/// single verification call.
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

/// End-to-end cache wiring: a successful first run records the
/// verification; a second run against the same lockfile +
/// trustworthy verifier policies hits the cache and never invokes
/// `verify` again.
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
async fn rejects_registry_style_key_backed_by_git_resolution_even_with_no_verifiers() {
    let lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      acme:
        specifier: ^1.0.0
        version: 1.0.0

packages:

  acme@1.0.0:
    resolution: {type: git, repo: https://example.com/acme.git, commit: 0123456789abcdef0123456789abcdef01234567}

snapshots:

  acme@1.0.0: {}
",
    );
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect_err("registry-style key with a git resolution must be rejected");
    let VerifyError::ResolutionShapeMismatch { count, breakdown } = err else {
        panic!("expected ResolutionShapeMismatch, got {err:?}");
    };
    assert_eq!(count, 1);
    assert!(breakdown.contains("acme@1.0.0"), "breakdown: {breakdown}");
}

#[tokio::test]
async fn accepts_artifact_keys_with_non_registry_resolutions() {
    let lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      acme:
        specifier: github:org/acme
        version: git+https://example.com/acme.git#0123456789abcdef0123456789abcdef01234567

packages:

  acme@git+https://example.com/acme.git#0123456789abcdef0123456789abcdef01234567:
    resolution: {type: git, repo: https://example.com/acme.git, commit: 0123456789abcdef0123456789abcdef01234567}
    version: 1.0.0

snapshots:

  acme@git+https://example.com/acme.git#0123456789abcdef0123456789abcdef01234567: {}
",
    );
    verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect("artifact-keyed git entry passes the structural gate");
}

#[tokio::test]
async fn rejects_git_host_tarball_when_git_hosted_flag_is_cleared() {
    // A tampered lockfile sets gitHosted: false on a codeload URL under a
    // semver key to dodge the flag-only check; the URL must still flag it.
    let lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      acme:
        specifier: ^1.0.0
        version: 1.0.0

packages:

  acme@1.0.0:
    resolution: {integrity: sha512-deadbeef, tarball: 'https://codeload.github.com/org/acme/tar.gz/abc123', gitHosted: false}

snapshots:

  acme@1.0.0: {}
",
    );
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect_err("git-host tarball under a semver key must be rejected regardless of the flag");
    let VerifyError::ResolutionShapeMismatch { count, .. } = err else {
        panic!("expected ResolutionShapeMismatch, got {err:?}");
    };
    assert_eq!(count, 1);
}

#[tokio::test]
async fn rejects_semver_key_backed_by_non_http_tarball() {
    // A file: tarball under a semver key is not registry-backed and the npm
    // verifier skips non-http(s) tarballs, so the shape pass must reject it.
    let lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      acme:
        specifier: ^1.0.0
        version: 1.0.0

packages:

  acme@1.0.0:
    resolution: {integrity: sha512-deadbeef, tarball: 'file:///tmp/evil.tgz'}

snapshots:

  acme@1.0.0: {}
",
    );
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect_err("file: tarball under a semver key must be rejected");
    assert!(matches!(err, VerifyError::ResolutionShapeMismatch { .. }), "got {err:?}");
}

#[tokio::test]
async fn rejects_git_host_tarball_with_uppercased_host() {
    // Hostnames are case-insensitive; an uppercased codeload host with
    // gitHosted: false must still be rejected under a semver key.
    let lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      acme:
        specifier: ^1.0.0
        version: 1.0.0

packages:

  acme@1.0.0:
    resolution: {integrity: sha512-deadbeef, tarball: 'https://CODELOAD.GITHUB.COM/org/acme/tar.gz/abc123', gitHosted: false}

snapshots:

  acme@1.0.0: {}
",
    );
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect_err("uppercased git-host tarball must be rejected");
    assert!(matches!(err, VerifyError::ResolutionShapeMismatch { .. }), "got {err:?}");
}
