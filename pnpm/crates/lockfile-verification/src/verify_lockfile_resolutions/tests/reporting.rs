use super::{
    AlwaysFail, Arc, FOUR_PKG_LOCKFILE, FailFor, LockfileVerificationMessage, LogEvent, Mutex,
    Path, Reporter, ResolutionVerifier, SINGLE_PKG_LOCKFILE, SilentReporter, TempDir,
    VerifyLockfileResolutionsOptions, parse, verify_lockfile_resolutions,
};
use pnpm_lockfile::LockfileResolution;
use pnpm_resolving_resolver_base::{ResolutionVerification, VerifyCtx, VerifyFuture};

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
        let delay = self
            .delays
            .iter()
            .find(|(delayed, _)| *delayed == name.as_str())
            .map_or(0, |(_, ms)| *ms);
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
            EVENTS.lock().unwrap().push(event.clone());
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
/// count the terminal `Done` already carries.
#[tokio::test]
async fn progress_events_are_throttled_between_started_and_done() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    EVENTS.lock().unwrap().clear();
    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
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
