use pretty_assertions::assert_eq;
use std::sync::atomic::AtomicU8;

/// `log_method_once` emits one `pnpm:package-import-method` event
/// per resolved method per `logged` atomic. Repeated calls with the
/// same flag are suppressed, distinct flags fire independently.
/// Production threads an install-scoped atomic from `Install::run`
/// down to [`link_file`]; this test passes a per-test atomic so it
/// observes the single-emit-per-method contract without racing
/// other tests.
#[test]
fn log_method_once_emits_first_call_per_method_only() {
    use pnpm_reporter::{LogEvent, PackageImportMethod as WireImportMethod, Reporter};
    use std::sync::Mutex;

    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    // Reset in case nextest reuses the process for a retry of this test.
    EVENTS.lock().unwrap().clear();

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let logged = AtomicU8::new(0);

    super::super::log_method_once::<RecordingReporter>(
        &logged,
        super::super::LOG_FLAG_CLONE,
        WireImportMethod::Clone,
    );
    super::super::log_method_once::<RecordingReporter>(
        &logged,
        super::super::LOG_FLAG_CLONE,
        WireImportMethod::Clone,
    );
    super::super::log_method_once::<RecordingReporter>(
        &logged,
        super::super::LOG_FLAG_HARDLINK,
        WireImportMethod::Hardlink,
    );

    let captured = EVENTS.lock().unwrap();
    let kinds: Vec<WireImportMethod> = captured
        .iter()
        .map(|event| match event {
            LogEvent::PackageImportMethod(log) => log.method,
            other => panic!("unexpected event {other:?}"),
        })
        .collect();
    assert_eq!(kinds, [WireImportMethod::Clone, WireImportMethod::Hardlink]);
}
