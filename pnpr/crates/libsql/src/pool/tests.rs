use crate::{
    LibsqlConnector,
    pool::ConnectionPool,
    testing::{SYNC_STALL, serve_primary},
};
use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use libsql::{Builder, Database};
use pnpr_error::RegistryError;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fmt::Write as _,
    sync::{Arc, Mutex, atomic::Ordering},
    time::Duration,
};

/// The error these pools fail with past their deadline: one the pool cannot
/// produce otherwise, so a test sees the owner's choice come back.
fn timed_out() -> RegistryError {
    RegistryError::StorageUnavailable { reason: "timed out".to_string() }
}

/// A remote connection whose request was cut off mid-flight sends the spent
/// baton next time, and the server refuses every later request on it.
#[tokio::test]
async fn a_connection_cut_off_mid_request_stays_broken() {
    let (db, server) = baton_checking_database().await;
    let conn = db.connect().unwrap();
    conn.query("SELECT 1", ()).await
        .unwrap();
    assert!(cut_off(conn.query("SELECT 'slow'", ())).await);

    let error = conn
        .query("SELECT 1", ())
        .await
        .unwrap_err();
    server.abort();
    assert!(error.to_string().contains("already been used"), "{error}");
}

#[tokio::test]
async fn the_pool_replaces_a_connection_cut_off_mid_request() {
    let (db, server) = baton_checking_database().await;
    let pool = ConnectionPool::new(db, 1, Duration::from_secs(30), timed_out);
    pool.read(|conn| async move { Ok(conn.query("SELECT 1", ()).await?) }).await
        .unwrap();
    let slow = pool.read(|conn| async move { Ok(conn.query("SELECT 'slow'", ()).await?) });
    assert!(cut_off(slow).await);

    let after = pool.read(|conn| async move { Ok(conn.query("SELECT 1", ()).await?) }).await;
    server.abort();
    after.unwrap();
}

/// A database that stops answering a read cannot keep the pool's connections:
/// the read fails at the deadline and the next operation gets a connection.
#[tokio::test]
async fn a_stalled_read_times_out_and_frees_its_connection() {
    let (db, server) = baton_checking_database().await;
    let pool = ConnectionPool::new(db, 1, Duration::from_millis(200), timed_out);
    let stalled = pool.read(|conn| async move { Ok(conn.query("SELECT 'slow'", ()).await?) }).await;
    assert!(matches!(stalled, Err(RegistryError::StorageUnavailable { .. })), "{stalled:?}");

    let after = pool.read(|conn| async move { Ok(conn.query("SELECT 1", ()).await?) }).await;
    server.abort();
    after.unwrap();
}

/// A write is never cancelled once sent, so its outcome is never unknown. Only
/// its wait for a connection is bounded.
#[tokio::test]
async fn a_write_outlives_the_deadline_but_its_wait_for_a_connection_does_not() {
    let (db, server) = baton_checking_database().await;
    let pool = ConnectionPool::new(db, 1, Duration::from_millis(200), timed_out);
    let slow = pool.write(|conn| async move { Ok(conn.query("SELECT 'slow'", ()).await?) });
    let queued = async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        pool.write(|conn| async move { Ok(conn.query("SELECT 1", ()).await?) }).await
    };
    let (slow, queued) = tokio::join!(slow, queued);
    server.abort();
    slow.unwrap();
    assert!(matches!(queued, Err(RegistryError::StorageUnavailable { .. })), "{queued:?}");
}

/// Whether `operation` was still waiting for its response when it was dropped.
async fn cut_off<Output>(operation: impl Future<Output = Output>) -> bool {
    tokio::time::timeout(Duration::from_millis(100), operation).await.is_err()
}

/// A database behind a Hrana server that hands out a fresh baton with every
/// response and refuses one it has seen before, as Turso and `sqld` do. A
/// statement containing `slow` gets its response late.
async fn baton_checking_database() -> (Database, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route("/v3/pipeline", post(pipeline))
        .route("/v3/cursor", post(cursor))
        .with_state(Arc::new(Batons::default()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let db = Builder::new_remote(format!("http://{address}"), String::new())
        .connector(tower::service_fn(move |_| tokio::net::TcpStream::connect(address)))
        .build()
        .await
        .unwrap();
    (db, server)
}

#[derive(Default)]
struct Batons {
    seen: Mutex<HashSet<String>>,
    issued: Mutex<u64>,
}

impl Batons {
    /// Spend `baton`, refusing one already spent.
    fn spend(&self, baton: &Value) -> Result<(), (StatusCode, &'static str)> {
        match baton.as_str() {
            Some(baton)
                if !self.seen
                    .lock()
                    .unwrap()
                    .insert(baton.to_string()) =>
            {
                Err((StatusCode::BAD_REQUEST, "Received a baton that has already been used"))
            }
            _ => Ok(()),
        }
    }

    fn issue(&self) -> String {
        let mut issued = self.issued.lock().unwrap();
        *issued += 1;
        format!("baton-{issued}")
    }
}

/// The `describe` that opens a `query`'s stream.
async fn pipeline(
    State(batons): State<Arc<Batons>>,
    body: String,
) -> Result<Json<Value>, (StatusCode, &'static str)> {
    let body: Value = serde_json::from_str(&body).unwrap();
    batons.spend(&body["baton"])?;
    let results: Vec<Value> = body["requests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|request| {
            assert_eq!(request["type"], "describe", "{request}");
            json!({ "type": "ok", "response": { "type": "describe", "result": {
                "params": [], "cols": [], "is_explain": false, "is_readonly": true,
            }}})
        })
        .collect();
    Ok(Json(json!({ "baton": batons.issue(), "base_url": null, "results": results })))
}

/// The rows of a `query`: one row holding `1`.
async fn cursor(
    State(batons): State<Arc<Batons>>,
    body: String,
) -> Result<String, (StatusCode, &'static str)> {
    let body: Value = serde_json::from_str(&body).unwrap();
    batons.spend(&body["baton"])?;
    if body.to_string().contains("slow") {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let lines = [
        json!({ "baton": batons.issue(), "base_url": null }),
        json!({ "type": "step_begin", "step": 0, "cols": [{ "name": "1", "decltype": null }] }),
        json!({ "type": "row", "row": [{ "type": "integer", "value": "1" }] }),
        json!({ "type": "step_end", "affected_row_count": 0, "last_insert_rowid": null }),
    ];
    let mut body = String::new();
    for line in lines {
        writeln!(body, "{line}").unwrap();
    }
    Ok(body)
}

/// Opening a replica connection waits for a background sync in progress. A
/// stalled sync must not hold an operation past its deadline, nor leave more
/// blocked opens behind than the pool has connections.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stalled_replica_sync_does_not_hold_operations_past_their_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let (primary, address, server) = serve_primary(directory.path().join("primary.db")).await;
    let url = format!("http://{address}");
    let db = Builder::new_remote_replica(
        directory.path().join("replica.db"),
        url.clone(),
        String::new(),
    )
    .connector(LibsqlConnector::for_url(&url, false).unwrap())
    .read_your_writes(false)
    .sync_interval(Duration::from_millis(200))
    .build()
    .await
    .unwrap();
    primary.sync_stall.enabled.store(true, Ordering::SeqCst);
    tokio::time::timeout(Duration::from_secs(5), primary.sync_stall.started.notified())
        .await
        .expect("a background sync reaches the stalled primary");

    let max_connections = 2;
    let pool = ConnectionPool::new(db, max_connections, Duration::from_millis(300), timed_out);
    let started = std::time::Instant::now();
    let read = || pool.read(|conn| async move { Ok(conn.query("SELECT 1", ()).await?) });
    let (first, second, third, fourth, fifth) = (read(), read(), read(), read(), read());
    let reads: [_; 5] = tokio::join!(first, second, third, fourth, fifth).into();
    let elapsed = started.elapsed();
    let in_flight = pool.opens_in_flight(max_connections);
    server.abort();
    for read in &reads {
        assert!(matches!(read, Err(RegistryError::StorageUnavailable { .. })), "{read:?}");
    }
    assert!(elapsed < SYNC_STALL / 2, "{elapsed:?}");
    assert_eq!(in_flight, max_connections, "opens still blocked once every read timed out");
}
