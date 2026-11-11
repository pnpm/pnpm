use super::{
    LibsqlAdapter, SyncStart, SyncState,
    statements::{
        StepKind, StreamState, TxStatus, classify_sql, script_transition, step_transition,
    },
};
use crate::adapter::Adapter;
use pnpr_config::LibsqlSettings;
use pnpr_error::RegistryError;
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
    let mut tx = TxStatus::Autocommit;
    let state = step_transition(StreamState::Active, &mut tx, StepKind::Begin);
    assert_eq!(tx, TxStatus::InTransaction);
    assert_eq!(state, StreamState::Active);
    assert_eq!(step_transition(state, &mut tx, StepKind::Commit), StreamState::Active);
    assert_eq!(tx, TxStatus::Autocommit);

    let state = step_transition(StreamState::Active, &mut tx, StepKind::Write);
    assert_eq!(state, StreamState::Committed);

    tx = TxStatus::InTransaction;
    let staged = step_transition(StreamState::Active, &mut tx, StepKind::Write);
    assert_eq!(staged, StreamState::StagedWrite);
    assert_eq!(step_transition(staged, &mut tx, StepKind::Write), StreamState::StagedWrite);
    assert_eq!(step_transition(staged, &mut tx, StepKind::Rollback), StreamState::Active);
    assert_eq!(tx, TxStatus::Autocommit);

    assert_eq!(
        script_transition(StreamState::Active, TxStatus::Autocommit, " ; "),
        StreamState::Active,
    );
    assert_eq!(
        script_transition(StreamState::Active, TxStatus::InTransaction, "UPDATE t SET x = 1;"),
        StreamState::StagedWrite,
    );
    assert_eq!(
        script_transition(StreamState::StagedWrite, TxStatus::InTransaction, "ROLLBACK;"),
        StreamState::Active,
    );
    assert_eq!(
        script_transition(
            StreamState::Active,
            TxStatus::Autocommit,
            "BEGIN; UPDATE t SET x = 1; COMMIT; BEGIN",
        ),
        StreamState::Committed,
    );
}

#[test]
fn unavailable_primary_errors_exclude_invalid_schema_and_protocol_errors() {
    let adapter = LibsqlAdapter::new(LibsqlSettings {
        url: "http://127.0.0.1:8080".to_string(),
        auth_token: None,
        replica_path: None,
        sync_interval_secs: None,
        max_connections: None,
    });
    for message in [
        "http error: `connection reset`",
        "api error: `status=503 Service Unavailable, body=busy`",
        "api error: `status=429 Too Many Requests, body=busy`",
    ] {
        let error = RegistryError::Libsql(libsql::Error::Hrana(message.into()));
        assert!(adapter.is_unavailable(&error), "{error}");
    }
    for message in [
        "api error: `SQLite failure: no such table: schema_migrations`",
        "api error: `status=401 Unauthorized, body=unauthorized`",
        "json error: `malformed response`",
    ] {
        let error = RegistryError::Libsql(libsql::Error::Hrana(message.into()));
        assert!(!adapter.is_unavailable(&error), "{error}");
    }
}
