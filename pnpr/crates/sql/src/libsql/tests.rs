use super::{
    SyncStart, SyncState,
    statements::{StepKind, StreamState, classify_sql, script_transition, step_transition},
};
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

#[test]
fn classify_sql_distinguishes_reads_transactions_rollbacks_and_writes() {
    assert_eq!(classify_sql(" -- note\n /* block */ SELECT 1"), StepKind::Read);
    assert_eq!(classify_sql("BEGIN IMMEDIATE"), StepKind::Begin);
    assert_eq!(classify_sql("COMMIT"), StepKind::Commit);
    assert_eq!(classify_sql("END TRANSACTION"), StepKind::Commit);
    assert_eq!(classify_sql("ROLLBACK"), StepKind::Rollback);
    assert_eq!(classify_sql("ROLLBACK TRANSACTION"), StepKind::Rollback);
    assert_eq!(classify_sql("ROLLBACK TO sp"), StepKind::Read);
    assert_eq!(classify_sql("ROLLBACK TRANSACTION TO sp"), StepKind::Read);
    assert_eq!(classify_sql("UPDATE catalogs SET generation = 1"), StepKind::Write);
}

#[test]
fn step_and_script_transitions_track_autocommit_and_staged_writes() {
    let mut in_tx = false;
    let state = step_transition(StreamState::Active, &mut in_tx, StepKind::Begin);
    assert!(in_tx);
    assert_eq!(state, StreamState::Active);
    assert_eq!(step_transition(state, &mut in_tx, StepKind::Commit), StreamState::Active);
    assert!(!in_tx);

    let state = step_transition(StreamState::Active, &mut in_tx, StepKind::Write);
    assert_eq!(state, StreamState::Committed);

    in_tx = true;
    let staged = step_transition(StreamState::Active, &mut in_tx, StepKind::Write);
    assert_eq!(staged, StreamState::StagedWrite);
    assert_eq!(step_transition(staged, &mut in_tx, StepKind::Rollback), StreamState::Active);
    assert!(!in_tx);

    assert_eq!(script_transition(StreamState::Active, false, false, " ; "), StreamState::Active);
    assert_eq!(
        script_transition(StreamState::Active, false, true, "BEGIN; UPDATE t SET x = 1;"),
        StreamState::StagedWrite,
    );
    assert_eq!(
        script_transition(StreamState::Active, false, true, "UPDATE t SET x = 1; BEGIN;"),
        StreamState::Committed,
    );
    assert_eq!(
        script_transition(StreamState::StagedWrite, true, false, "UPDATE t SET x = 1; COMMIT;"),
        StreamState::Committed,
    );
}
