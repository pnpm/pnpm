use super::{
    AlwaysFail,
    Arc,
    FailFor,
    LockfileVerificationMessage,
    LogEvent,
    Mutex,
    Path,
    Reporter,
    ResolutionVerifier,
    SINGLE_PKG_LOCKFILE,
    SilentReporter,
    TWO_PKG_LOCKFILE,
    TempDir,
    VerifyError,
    VerifyLockfileResolutionsOptions,
    collect_candidates,
    parse,
    run_fan_out,
    verify_lockfile_resolutions,
};
use pnpm_lockfile::LockfileResolution;
use pnpm_resolving_resolver_base::{
    ResolutionVerification,
    VerifyCtx,
    VerifyFuture,
};

const FOUR_PKG_LOCKFILE: &str = "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      acme:
        specifier: ^1.0.0
        version: 1.0.0
      bravo:
        specifier: ^2.0.0
        version: 2.0.0
      charlie:
        specifier: ^3.0.0
        version: 3.0.0
      delta:
        specifier: ^4.0.0
        version: 4.0.0

packages:

  acme@1.0.0:
    resolution: {tarball: 'https://registry.npmjs.org/acme/-/acme-1.0.0.tgz'}

  bravo@2.0.0:
    resolution: {tarball: 'https://registry.npmjs.org/bravo/-/bravo-2.0.0.tgz'}

  charlie@3.0.0:
    resolution: {tarball: 'https://registry.npmjs.org/charlie/-/charlie-3.0.0.tgz'}

  delta@4.0.0:
    resolution: {tarball: 'https://registry.npmjs.org/delta/-/delta-4.0.0.tgz'}

snapshots:

  acme@1.0.0: {}
  bravo@2.0.0: {}
  charlie@3.0.0: {}
  delta@4.0.0: {}
";

/// Pass everything, completing each named package after its listed
/// millisecond delay (packages not listed complete immediately), so a
/// test controls when entries finish.
struct DelayedOk {
    delays: Vec<(&'static str, u64)>,
    policy: serde_json::Map<String, serde_json::Value>,
}

impl DelayedOk {
    fn new(delays: Vec<(&'static str, u64)>) -> Arc<Self> {
        Arc::new(Self { delays, policy: serde_json::Map::new() })
    }
}

impl ResolutionVerifier for DelayedOk {
    fn verify<'a>(
        &'a self,
        _resolution: &'a LockfileResolution,
        ctx: VerifyCtx<'a>,
    ) -> VerifyFuture<'a> {
        let name = ctx.name.to_string();
        let delay = self.delays
            .iter()
            .find(|(delayed, _)| *delayed == name.as_str())
            .map(|(_, ms)| *ms)
            .unwrap_or(0);
        Box::pin(async move {
            if delay > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            }
            ResolutionVerification::Ok
        })
    }

    fn policy(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.policy
    }

    fn can_trust_past_check(&self, _cached: &serde_json::Map<String, serde_json::Value>) -> bool {
        true
    }
}

#[tokio::test]
async fn all_ok_emits_started_then_done() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();
    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS
                .lock()
                .unwrap()
                .push(event.clone());
        }
    }

    let lockfile = parse(SINGLE_PKG_LOCKFILE);
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

#[tokio::test]
async fn failed_path_emits_failed_terminator() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();
    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS
                .lock()
                .unwrap()
                .push(event.clone());
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
            matches!(
                log.message,
                // The fan-out ran to completion before collecting the
                // violation, so the failure reports the full count.
                LockfileVerificationMessage::Failed { entries: 1, checked: 1, .. }
            ),
            "expected Failed with checked == entries, got: {:?}",
            log.message,
        ),
        other => panic!("expected LockfileVerification, got {other:?}"),
    }
}

#[tokio::test]
async fn cache_hit_emits_cached_event() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();
    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS
                .lock()
                .unwrap()
                .push(event.clone());
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
    verify_lockfile_resolutions::<RecordingReporter>(
        &lockfile,
        std::slice::from_ref(&verifier),
        &opts,
    )
    .await
    .expect("second run");

    let captured = EVENTS.lock().unwrap();
    assert_eq!(captured.len(), 1, "expected a single Cached event, got: {captured:?}");
    match &captured[0] {
        LogEvent::LockfileVerification(log) => match &log.message {
            LockfileVerificationMessage::Cached { verified_at, lockfile_path: emitted } => {
                assert_eq!(emitted.as_deref(), Some(&*lockfile_path.to_string_lossy()));
                assert!(
                    verified_at.is_some(),
                    "the reused record carries the first run's timestamp",
                );
            }
            other => panic!("expected Cached, got {other:?}"),
        },
        other => panic!("expected LockfileVerification, got {other:?}"),
    }
}

/// acme finishes immediately (inside the first progress interval, so
/// its completion is throttled away); bravo lands after the interval
/// and is reported; charlie lands within the interval that follows
/// bravo's report and is throttled away; delta completes the run, a
/// count the terminal `Done` already carries. The clock is paused, so
/// the delays elapse exactly.
#[tokio::test(start_paused = true)]
async fn progress_events_are_throttled_between_started_and_done() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();
    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS
                .lock()
                .unwrap()
                .push(event.clone());
        }
    }

    let lockfile = parse(FOUR_PKG_LOCKFILE);
    let verifier = DelayedOk::new(vec![("bravo", 450), ("charlie", 470), ("delta", 490)]);
    verify_lockfile_resolutions::<RecordingReporter>(
        &lockfile,
        &[verifier as Arc<dyn ResolutionVerifier>],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect("all-ok must succeed");

    let captured = EVENTS.lock().unwrap();
    assert_eq!(captured.len(), 3, "expected Started + Progress + Done, got: {captured:?}");
    match &captured[1] {
        LogEvent::LockfileVerification(log) => assert!(
            matches!(
                log.message,
                LockfileVerificationMessage::Progress { entries: 4, checked: 2, .. }
            ),
            "expected one Progress reporting checked == 2, got: {:?}",
            log.message,
        ),
        other => panic!("expected LockfileVerification, got {other:?}"),
    }
    match &captured[2] {
        LogEvent::LockfileVerification(log) => assert!(
            matches!(log.message, LockfileVerificationMessage::Done { checked: 4, .. }),
            "expected Done carrying the full count, got: {:?}",
            log.message,
        ),
        other => panic!("expected LockfileVerification, got {other:?}"),
    }
}

/// Ignores every resolution (`might_verify` returns false), the way a
/// verifier scoped to one resolution kind skips the rest.
struct MatchesNone {
    policy: serde_json::Map<String, serde_json::Value>,
}

impl MatchesNone {
    fn new() -> Arc<Self> {
        Arc::new(Self { policy: serde_json::Map::new() })
    }
}

impl ResolutionVerifier for MatchesNone {
    fn might_verify(&self, _resolution: &LockfileResolution, _ctx: VerifyCtx<'_>) -> bool {
        false
    }

    fn verify<'a>(
        &'a self,
        _resolution: &'a LockfileResolution,
        _ctx: VerifyCtx<'a>,
    ) -> VerifyFuture<'a> {
        Box::pin(async { ResolutionVerification::Ok })
    }

    fn policy(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.policy
    }

    fn can_trust_past_check(&self, _cached: &serde_json::Map<String, serde_json::Value>) -> bool {
        true
    }
}

#[tokio::test]
async fn unmatched_candidates_are_reported_as_completed() {
    let lockfile = parse(TWO_PKG_LOCKFILE);
    assert!(
        crate::collect_resolution_shape_violations(&lockfile).is_empty(),
        "fixture lockfile must be shape-clean",
    );
    let candidates = collect_candidates(&lockfile);
    let verifier = MatchesNone::new();
    let mut checked = 0;
    let violations =
        run_fan_out(candidates, &[verifier as Arc<dyn ResolutionVerifier>], None, &mut || {
            checked += 1;
        })
        .await
        .expect("no transport failures");
    assert!(violations.is_empty());
    assert_eq!(checked, 2, "both unmatched candidates reach the callback");
}

/// Transport-fails the named package after a short delay; other
/// candidates pass, so the failure lands mid-pass rather than first.
struct FetchFailsFor {
    name: &'static str,
    policy: serde_json::Map<String, serde_json::Value>,
}

impl FetchFailsFor {
    fn new(name: &'static str) -> Arc<Self> {
        Arc::new(Self { name, policy: serde_json::Map::new() })
    }
}

impl ResolutionVerifier for FetchFailsFor {
    fn verify<'a>(
        &'a self,
        _resolution: &'a LockfileResolution,
        ctx: VerifyCtx<'a>,
    ) -> VerifyFuture<'a> {
        let triggers = ctx.name.to_string() == self.name;
        let message = "registry unreachable".to_string();
        Box::pin(async move {
            if triggers {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                ResolutionVerification::FetchFailed { message }
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

#[tokio::test]
async fn fetch_failure_reports_the_observed_checked_count() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();
    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS
                .lock()
                .unwrap()
                .push(event.clone());
        }
    }

    let lockfile = parse(TWO_PKG_LOCKFILE);
    let verifier = FetchFailsFor::new("bravo");
    let result = verify_lockfile_resolutions::<RecordingReporter>(
        &lockfile,
        &[verifier as Arc<dyn ResolutionVerifier>],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await;
    match result {
        Err(VerifyError::RegistryMetaFetchFailed { .. }) => {}
        other => panic!("expected a registry-fetch abort, got {other:?}"),
    }

    let captured = EVENTS.lock().unwrap();
    assert_eq!(captured.len(), 2, "expected Started + Failed, got: {captured:?}");
    match &captured[1] {
        LogEvent::LockfileVerification(log) => assert!(
            matches!(
                log.message,
                // acme passed before the abort and was counted; bravo's
                // transport failure stopped the count there.
                LockfileVerificationMessage::Failed { entries: 2, checked: 1, .. }
            ),
            "expected Failed carrying the observed count, got: {:?}",
            log.message,
        ),
        other => panic!("expected LockfileVerification, got {other:?}"),
    }
}

/// Panics the named package after a short delay so other candidates pass first.
struct PanicsFor {
    name: &'static str,
    policy: serde_json::Map<String, serde_json::Value>,
}

impl PanicsFor {
    fn new(name: &'static str) -> Arc<Self> {
        Arc::new(Self { name, policy: serde_json::Map::new() })
    }
}

impl ResolutionVerifier for PanicsFor {
    fn verify<'a>(
        &'a self,
        _resolution: &'a LockfileResolution,
        ctx: VerifyCtx<'a>,
    ) -> VerifyFuture<'a> {
        let triggers = ctx.name.to_string() == self.name;
        Box::pin(async move {
            if triggers {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                panic!("intentional panic during verification");
            }
            ResolutionVerification::Ok
        })
    }

    fn policy(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.policy
    }

    fn can_trust_past_check(&self, _cached: &serde_json::Map<String, serde_json::Value>) -> bool {
        true
    }
}

#[tokio::test]
async fn panic_drop_guard_reports_observed_checked_count() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();
    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS
                .lock()
                .unwrap()
                .push(event.clone());
        }
    }

    let lockfile = parse(TWO_PKG_LOCKFILE);
    let verifier = PanicsFor::new("bravo");
    let handle = tokio::spawn(async move {
        verify_lockfile_resolutions::<RecordingReporter>(
            &lockfile,
            &[verifier as Arc<dyn ResolutionVerifier>],
            &VerifyLockfileResolutionsOptions::default(),
        )
        .await
    });
    let result = handle.await;
    assert!(result.is_err(), "tokio task must return join error on panic");

    let captured = EVENTS.lock().unwrap();
    assert_eq!(captured.len(), 2, "expected Started + Failed, got: {captured:?}");
    match &captured[1] {
        LogEvent::LockfileVerification(log) => assert!(
            matches!(
                log.message,
                LockfileVerificationMessage::Failed { entries: 2, checked: 1, .. }
            ),
            "expected Failed carrying the observed count 1, got: {:?}",
            log.message,
        ),
        other => panic!("expected LockfileVerification, got {other:?}"),
    }
}
