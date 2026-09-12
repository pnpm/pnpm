use super::{super::removed_child_aliases, name, snapshot};
use crate::create_virtual_store::slot_linking::emit_warm_snapshot_progress;
use pnpm_lockfile::PkgName;
use pnpm_reporter::{LogEvent, ProgressMessage, Reporter};
use std::sync::Mutex;

#[test]
fn removed_child_aliases_reports_dropped_children_only() {
    let self_name = name("host");
    let current = snapshot(&["kept", "dropped"], &["opt-dropped"]);
    let wanted = snapshot(&["kept", "added"], &[]);

    let mut removed: Vec<String> = removed_child_aliases(&current, &wanted, &self_name)
        .iter()
        .map(PkgName::to_string)
        .collect();
    removed.sort();

    assert_eq!(removed, vec!["dropped".to_string(), "opt-dropped".to_string()]);
}
/// `emit_warm_snapshot_progress` fires `resolved` then
/// `found_in_store` when no earlier fetch path already emitted the
/// package status. Both events carry the same identifiers — pnpm's
/// per-package counter relies on the pair to pin the tick to the right
/// package row.
#[test]
fn emits_resolved_then_found_in_store_when_not_progress_reported() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    EVENTS.lock().unwrap().clear();
    emit_warm_snapshot_progress::<RecordingReporter>("react@18.0.0", "/proj", false);

    let captured = EVENTS.lock().unwrap();
    assert!(
        matches!(
            captured.as_slice(),
            [
                LogEvent::Progress(r),
                LogEvent::Progress(f),
            ] if matches!(
                &r.message,
                ProgressMessage::Resolved { package_id, requester }
                    if package_id == "react@18.0.0" && requester == "/proj"
            ) && matches!(
                &f.message,
                ProgressMessage::FoundInStore { package_id, requester }
                    if package_id == "react@18.0.0" && requester == "/proj",
            ),
        ),
        "warm-snapshot pair must be (Resolved, FoundInStore) with matching identifiers; got {captured:?}",
    );
}
/// When an earlier fetch path already emitted `fetched` or
/// `found_in_store`, the warm batch emits only `resolved` so the
/// package status is not double-counted. Regression guard for
/// <https://github.com/pnpm/pnpm/issues/12235>.
#[test]
fn emits_only_resolved_when_progress_reported() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    EVENTS.lock().unwrap().clear();
    emit_warm_snapshot_progress::<RecordingReporter>("react@18.0.0", "/proj", true);

    let captured = EVENTS.lock().unwrap();
    assert!(
        matches!(
            captured.as_slice(),
            [LogEvent::Progress(r)] if matches!(
                &r.message,
                ProgressMessage::Resolved { package_id, requester }
                    if package_id == "react@18.0.0" && requester == "/proj"
            ),
        ),
        "already-reported warm snapshot must report only Resolved; got {captured:?}",
    );
}
