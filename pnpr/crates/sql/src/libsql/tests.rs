use super::{SyncStart, SyncState};
use std::time::Duration;

const BOUND: Duration = Duration::from_hours(1);

#[test]
fn a_sync_that_a_write_overtook_leaves_the_replica_stale() {
    let mut state = SyncState::default();
    let started = state.start_sync();
    // The write commits while the sync pulls a snapshot that may predate it.
    state.mark_stale();
    state.finish_sync(started);
    assert!(!state.is_fresh(BOUND));
    // The next sync starts after the write, so its pull holds it.
    let started = state.start_sync();
    state.finish_sync(started);
    assert!(state.is_fresh(BOUND));
}

#[test]
fn a_write_after_a_sync_leaves_the_replica_stale() {
    let mut state = SyncState::default();
    let started = state.start_sync();
    state.finish_sync(started);
    assert!(state.is_fresh(BOUND));
    state.mark_stale();
    assert!(!state.is_fresh(BOUND));
}

#[test]
fn a_sync_started_before_a_request_does_not_answer_it() {
    let mut state = SyncState::default();
    let started = state.start_sync();
    // Later than the start even where two readings of the clock can tie.
    let requested = started.at + Duration::from_millis(1);
    state.finish_sync(started);
    assert!(!state.synced_since(requested));
    let started = SyncStart { at: requested, ..state.start_sync() };
    state.finish_sync(started);
    assert!(state.synced_since(requested));
}
