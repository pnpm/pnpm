//! An embedded replica against a primary that serves libsql's newer sync
//! protocol, as Turso Cloud does. libsql discovers that protocol through
//! the primary's `/info` route, bootstraps the replica from `/export`,
//! pulls frames from `/sync`, and sends writes to the primary over Hrana.
//! The primary here is a local libsql database behind a mock of those
//! routes that serves the frames of its own WAL.

use super::{
    super::{connector::LibsqlConnector, pool::ConnectionPool},
    Builder, LibsqlAuth, MaxUsers, RegistryError, TokenBackend, UpsertOutcome, UserBackend,
    is_unique_violation,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use libsql::{Connection, Database, Value};
use pnpr_config::LibsqlSettings;
use serde_json::{Value as JsonValue, json};
use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

/// The layout of a `SQLite` WAL file, whose frames the sync protocol serves
/// one generation per checkpoint.
const WAL_HEADER_SIZE: usize = 32;
const WAL_FRAME_SIZE: usize = 24 + 4096;

/// How long a stalled `/sync` takes to answer.
const SYNC_STALL: Duration = Duration::from_secs(3);

struct Primary {
    _db: Database,
    conn: Connection,
    path: PathBuf,
    /// Exports so far; each one checkpoints the WAL.
    exports: AtomicUsize,
    pipelines: AtomicUsize,
    /// The sync generation. `SQLite` restarts the WAL at frame 1 with a new
    /// salt after a checkpoint, which the protocol expresses as the next
    /// generation.
    generation: AtomicUsize,
    salt: std::sync::Mutex<Option<[u8; 8]>>,
    sync_stall: SyncStall,
}

/// A `/sync` that stops answering, as a primary on a broken network does.
#[derive(Default)]
struct SyncStall {
    enabled: AtomicBool,
    /// Signalled when a `/sync` request starts stalling.
    started: tokio::sync::Notify,
}

/// A primary at `path`, served on a loopback port.
async fn serve_primary(path: PathBuf) -> (Arc<Primary>, SocketAddr, tokio::task::JoinHandle<()>) {
    let db = Builder::new_local(&path).build().await.unwrap();
    let conn = db.connect().unwrap();
    // A primary keeps a WAL for the frames it serves; a plain local database
    // runs with a rollback journal.
    let mut journal_mode = conn
        .query("PRAGMA journal_mode = WAL", ())
        .await
        .unwrap();
    while journal_mode
        .next()
        .await
        .unwrap()
        .is_some()
    {}
    let primary = Arc::new(Primary {
        _db: db,
        conn,
        path,
        exports: AtomicUsize::new(0),
        pipelines: AtomicUsize::new(0),
        generation: AtomicUsize::new(1),
        salt: std::sync::Mutex::new(None),
        sync_stall: SyncStall::default(),
    });
    let app = Router::new()
        .route("/info", get(info))
        .route("/export/{generation}", get(export))
        .route("/sync/{generation}/{start}/{end}", get(sync))
        .route("/v3/pipeline", post(pipeline))
        .with_state(Arc::clone(&primary));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (primary, address, server)
}

fn replica_settings(
    address: SocketAddr,
    replica_path: PathBuf,
    sync_interval_secs: Option<u64>,
) -> LibsqlSettings {
    LibsqlSettings {
        url: format!("http://{address}"),
        auth_token: None,
        replica_path: Some(replica_path),
        sync_interval_secs,
        max_connections: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn embedded_replica_follows_a_primary_on_the_newer_sync_protocol() {
    let directory = tempfile::tempdir().unwrap();
    let (primary, address, server) = serve_primary(directory.path().join("primary.db")).await;
    let db = Builder::new_local(&primary.path).build().await.unwrap();
    let backend = LibsqlAuth::from_database(db, MaxUsers::Unlimited, 4).await.unwrap();
    backend.add_or_login("alice", "secret").await.unwrap();

    for (index, sync_interval_secs) in [Some(1), Some(0)].into_iter().enumerate() {
        let token = backend.issue("alice").await.unwrap();
        let replica_path = directory
            .path()
            .join(format!("replica{index}"))
            .join("auth.db");
        let settings = replica_settings(address, replica_path, sync_interval_secs);
        let pipelines_before_connect = primary.pipelines.load(Ordering::SeqCst);
        let replica = LibsqlAuth::connect(&settings, MaxUsers::Unlimited).await.unwrap();

        assert_eq!(
            primary.exports.load(Ordering::SeqCst),
            index + 1,
            "bootstrapped from the primary's export",
        );
        let pipelines_after_connect = primary.pipelines.load(Ordering::SeqCst);
        assert!(
            pipelines_after_connect > pipelines_before_connect,
            "schema setup writes go to the primary",
        );

        let owner = replica.lookup(&token).await.unwrap();
        assert_eq!(owner.as_deref(), Some("alice"));
        let hash = replica.stored_hash("alice").await.unwrap();
        assert!(hash.is_some());
        assert_eq!(
            primary.pipelines.load(Ordering::SeqCst),
            pipelines_after_connect,
            "reads are served from the local replica",
        );

        let issued = replica.issue("alice").await.unwrap();
        assert!(
            primary.pipelines.load(Ordering::SeqCst) > pipelines_after_connect,
            "writes go to the primary",
        );
        let owner = backend.lookup(&issued).await.unwrap();
        assert_eq!(owner.as_deref(), Some("alice"));
        let owner = replica.lookup(&issued).await.unwrap();
        assert_eq!(owner.as_deref(), Some("alice"), "own write visible");

        let mut tokens = vec![token, issued];
        if sync_interval_secs == Some(1) {
            let fresh = backend.issue("alice").await.unwrap();
            let owner = wait_for_token(&replica, &fresh).await;
            assert_eq!(owner.as_deref(), Some("alice"), "background sync pulls the write");
            tokens.push(fresh);
        }

        let revoked = replica.revoke_all_for_user("alice").await.unwrap();
        assert_eq!(revoked, tokens.len());
        for token in &tokens {
            assert_eq!(backend.lookup(token).await.unwrap(), None);
            assert_eq!(replica.lookup(token).await.unwrap(), None, "own bulk revoke visible");
        }
    }
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn embedded_replica_sets_up_the_schema_on_an_empty_primary() {
    let directory = tempfile::tempdir().unwrap();
    let (primary, address, server) = serve_primary(directory.path().join("primary.db")).await;
    let replica_path = directory
        .path()
        .join("replica")
        .join("auth.db");
    let settings = replica_settings(address, replica_path, None);
    let replica = LibsqlAuth::connect(&settings, MaxUsers::Limited(2)).await.unwrap();

    let (outcome, _) = replica.add_or_login("carol", "secret").await.unwrap();
    assert!(matches!(outcome, UpsertOutcome::Created));
    let token = replica.issue("carol").await.unwrap();
    let owner = replica.lookup(&token).await.unwrap();
    assert_eq!(owner.as_deref(), Some("carol"));

    let mut rows = primary.conn
        .query("SELECT value FROM auth_counters WHERE name = 'users'", ())
        .await
        .unwrap();
    let counter: i64 = rows
        .next()
        .await
        .unwrap()
        .unwrap()
        .get(0)
        .unwrap();
    assert_eq!(counter, 1, "the capped registration claimed a slot on the primary");
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bulk_revocation_refreshes_a_replica_whose_tokens_went_elsewhere() {
    let directory = tempfile::tempdir().unwrap();
    let (primary, address, server) = serve_primary(directory.path().join("primary.db")).await;
    let db = Builder::new_local(&primary.path).build().await.unwrap();
    let backend = LibsqlAuth::from_database(db, MaxUsers::Unlimited, 4).await.unwrap();
    backend.add_or_login("alice", "secret").await.unwrap();
    let replica_path = directory
        .path()
        .join("replica")
        .join("auth.db");
    let settings = replica_settings(address, replica_path, Some(0));
    let replica = LibsqlAuth::connect(&settings, MaxUsers::Unlimited).await.unwrap();

    let stale = backend.issue("alice").await.unwrap();
    replica.issue("alice").await.unwrap();
    assert_eq!(
        replica
            .lookup(&stale)
            .await
            .unwrap()
            .as_deref(),
        Some("alice"),
    );
    assert_eq!(backend.revoke_all_for_user("alice").await.unwrap(), 2);

    let revoked = replica.revoke_all_for_user("alice").await.unwrap();
    assert_eq!(revoked, 0, "the primary had nothing left to revoke");
    assert_eq!(replica.lookup(&stale).await.unwrap(), None, "the replica caught up");
    server.abort();
}

/// Poll `replica` for `token` until it is visible or the wait runs out.
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
    let pool = ConnectionPool::new(db, max_connections, Duration::from_millis(300));
    let started = std::time::Instant::now();
    let read = || pool.read(|conn| async move { Ok(conn.query("SELECT 1", ()).await?) });
    let (first, second, third, fourth, fifth) = (read(), read(), read(), read(), read());
    let reads: [_; 5] = tokio::join!(first, second, third, fourth, fifth).into();
    let elapsed = started.elapsed();
    let in_flight = pool.opens_in_flight(max_connections);
    server.abort();
    for read in &reads {
        assert!(matches!(read, Err(RegistryError::AuthDatabaseTimeout)), "{read:?}");
    }
    assert!(elapsed < SYNC_STALL / 2, "{elapsed:?}");
    assert_eq!(in_flight, max_connections, "opens still blocked once every read timed out");
}

async fn wait_for_token(replica: &LibsqlAuth, token: &str) -> Option<String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let owner = replica.lookup(token).await.unwrap();
        if owner.is_some() || tokio::time::Instant::now() > deadline {
            return owner;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn info(State(primary): State<Arc<Primary>>) -> Json<JsonValue> {
    let (generation, _) = primary.wal_state();
    Json(json!({ "current_generation": generation }))
}

async fn export(State(primary): State<Arc<Primary>>) -> Vec<u8> {
    let mut checkpoint = primary.conn
        .query("PRAGMA wal_checkpoint(TRUNCATE)", ())
        .await
        .unwrap();
    while checkpoint
        .next()
        .await
        .unwrap()
        .is_some()
    {}
    primary.exports.fetch_add(1, Ordering::SeqCst);
    std::fs::read(&primary.path).unwrap()
}

/// WAL frames `[start, end)` of `generation`, or the generation to move to.
async fn sync(
    State(primary): State<Arc<Primary>>,
    Path((generation, start, end)): Path<(usize, usize, usize)>,
) -> Result<Vec<u8>, (StatusCode, Json<JsonValue>)> {
    if primary.sync_stall.enabled.load(Ordering::SeqCst) {
        primary.sync_stall.started.notify_one();
        tokio::time::sleep(SYNC_STALL).await;
    }
    let (current, wal) = primary.wal_state();
    let frames = if generation == current { wal_frames(&wal, start, end) } else { None };
    frames.ok_or_else(|| (StatusCode::BAD_REQUEST, Json(json!({ "generation": current }))))
}

async fn pipeline(State(primary): State<Arc<Primary>>, body: String) -> Json<JsonValue> {
    primary.pipelines.fetch_add(1, Ordering::SeqCst);
    let body: JsonValue = serde_json::from_str(&body).unwrap();
    let mut results = Vec::new();
    for request in body["requests"].as_array().unwrap() {
        results.push(primary.answer(request).await);
    }
    Json(json!({ "baton": "primary", "base_url": null, "results": results }))
}

/// Frames `[start, end)` of `wal`, clipped to the frames it holds; `None`
/// past its end. A restarted WAL keeps the frames of its previous salt
/// behind the new ones, so only the leading frames carrying the header's
/// salt count.
fn wal_frames(wal: &[u8], start: usize, end: usize) -> Option<Vec<u8>> {
    let salt = wal.get(16..24)?;
    let frame_offset = |frame: usize| WAL_HEADER_SIZE + (frame - 1) * WAL_FRAME_SIZE;
    let frame_salt = |frame: usize| wal.get(frame_offset(frame) + 8..frame_offset(frame) + 16);
    let available = (1..=wal.len() / WAL_FRAME_SIZE)
        .take_while(|&frame| frame_salt(frame) == Some(salt))
        .count();
    if start == 0 || start > available {
        return None;
    }
    let end = end.min(available + 1);
    Some(wal[frame_offset(start)..frame_offset(end)].to_vec())
}

impl Primary {
    /// The current generation and the WAL holding its frames.
    fn wal_state(&self) -> (usize, Vec<u8>) {
        let mut wal_path = self.path.clone().into_os_string();
        wal_path.push("-wal");
        let wal = std::fs::read(wal_path).unwrap_or_default();
        let mut salt = self.salt.lock().unwrap();
        let header_salt = wal
            .get(16..24)
            .map(|bytes| <[u8; 8]>::try_from(bytes).unwrap());
        if header_salt.is_some() && header_salt != *salt {
            if salt.is_some() {
                self.generation.fetch_add(1, Ordering::SeqCst);
            }
            *salt = header_salt;
        }
        (self.generation.load(Ordering::SeqCst), wal)
    }

    async fn answer(&self, request: &JsonValue) -> JsonValue {
        match request["type"].as_str() {
            Some("batch") => self.run_batch(request["batch"]["steps"].as_array().unwrap()).await,
            Some("get_autocommit") => json!({
                "type": "ok",
                "response": {
                    "type": "get_autocommit",
                    "is_autocommit": self.conn.is_autocommit(),
                },
            }),
            Some("describe") => json!({
                "type": "ok",
                "response": {
                    "type": "describe",
                    "result": { "params": [], "cols": [], "is_explain": false, "is_readonly": false },
                },
            }),
            Some("close") => json!({ "type": "ok", "response": { "type": "close" } }),
            other => json!({
                "type": "error",
                "error": { "message": format!("unsupported request {other:?}"), "code": "UNSUPPORTED" },
            }),
        }
    }

    async fn run_batch(&self, steps: &[JsonValue]) -> JsonValue {
        let mut step_results = Vec::new();
        let mut step_errors = Vec::new();
        for step in steps {
            match self.run(&step["stmt"]).await {
                Ok(result) => {
                    step_results.push(result);
                    step_errors.push(JsonValue::Null);
                }
                Err(error) => {
                    step_results.push(JsonValue::Null);
                    step_errors.push(hrana_error(&error));
                }
            }
        }
        json!({
            "type": "ok",
            "response": {
                "type": "batch",
                "result": { "step_results": step_results, "step_errors": step_errors },
            },
        })
    }

    async fn run(&self, stmt: &JsonValue) -> libsql::Result<JsonValue> {
        let sql = stmt["sql"].as_str().unwrap();
        let args: Vec<Value> = stmt["args"]
            .as_array()
            .map(|args| args.iter().map(hrana_arg).collect())
            .unwrap_or_default();
        let mut rows = self.conn.query(sql, args).await?;
        let cols: Vec<JsonValue> = (0..rows.column_count())
            .map(|index| json!({ "name": rows.column_name(index), "decltype": null }))
            .collect();
        let mut encoded_rows = Vec::new();
        while let Some(row) = rows.next().await? {
            let values: Vec<JsonValue> = (0..row.column_count())
                .map(|index| hrana_value(row.get_value(index).unwrap()))
                .collect();
            encoded_rows.push(JsonValue::Array(values));
        }
        Ok(json!({
            "cols": cols,
            "rows": encoded_rows,
            "affected_row_count": self.conn.changes(),
            "last_insert_rowid": null,
        }))
    }
}

fn hrana_arg(arg: &JsonValue) -> Value {
    match arg["type"].as_str() {
        Some("null") => Value::Null,
        Some("integer") => Value::Integer(
            arg["value"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap(),
        ),
        Some("float") => Value::Real(arg["value"].as_f64().unwrap()),
        Some("text") => Value::Text(
            arg["value"]
                .as_str()
                .unwrap()
                .to_string(),
        ),
        other => panic!("unsupported Hrana argument type {other:?}"),
    }
}

fn hrana_value(value: Value) -> JsonValue {
    match value {
        Value::Null => json!({ "type": "null" }),
        Value::Integer(value) => json!({ "type": "integer", "value": value.to_string() }),
        Value::Real(value) => json!({ "type": "float", "value": value }),
        Value::Text(value) => json!({ "type": "text", "value": value }),
        Value::Blob(_) => panic!("the auth schema has no blob columns"),
    }
}

/// Turso reports a failed statement with the `SQLite` result code name.
fn hrana_error(error: &libsql::Error) -> JsonValue {
    let code = if is_unique_violation(error) { "SQLITE_CONSTRAINT" } else { "SQLITE_ERROR" };
    json!({ "message": error.to_string(), "code": code })
}
