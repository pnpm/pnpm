//! A local primary that serves libSQL's newer sync protocol, as Turso Cloud
//! does. libsql discovers that protocol through the primary's `/info` route,
//! bootstraps an embedded replica from `/export`, pulls frames from `/sync`,
//! and sends writes to the primary over Hrana. The primary here is a local
//! libsql database behind a mock of those routes that serves the frames of
//! its own WAL.

pub use plain_http::{EXECUTE_RESPONSE, PlainHttpServer, serve_plain_http};

mod plain_http;

use crate::{is_transaction_conflict, is_unique_violation};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD as BASE64};
use libsql::{Builder, Connection, Database, Value};
use serde_json::{Value as JsonValue, json};
use std::{
    collections::HashMap,
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
pub const SYNC_STALL: Duration = Duration::from_secs(3);

/// The primary database and what it has served.
pub struct Primary {
    streams: Streams,
    /// The primary's own connection, for a test to read or write it directly.
    pub conn: Connection,
    pub path: PathBuf,
    /// Exports so far; each one checkpoints the WAL.
    pub exports: AtomicUsize,
    /// Hrana pipeline and cursor requests so far: the round trips to the
    /// primary.
    pub pipelines: AtomicUsize,
    /// The statements of each of those requests, in order, for a test to
    /// report what a round trip carried.
    pub round_trips: std::sync::Mutex<Vec<Vec<String>>>,
    /// The sync generation. `SQLite` restarts the WAL at frame 1 with a new
    /// salt after a checkpoint, which the protocol expresses as the next
    /// generation.
    generation: AtomicUsize,
    salt: std::sync::Mutex<Option<[u8; 8]>>,
    pub sync_stall: SyncStall,
}

/// The connection of each open Hrana stream, by baton, so concurrent clients
/// hold transactions of their own, as on a real primary.
struct Streams {
    db: Database,
    open: std::sync::Mutex<HashMap<String, Connection>>,
    batons: AtomicUsize,
}

impl Streams {
    /// The connection of the stream `baton` names, or of a new stream.
    async fn take(&self, baton: Option<&str>) -> Connection {
        let open = baton.and_then(|baton| self.open.lock().unwrap().remove(baton));
        if let Some(conn) = open {
            return conn;
        }
        let conn = self.db.connect().unwrap();
        // A writer that finds another's transaction open waits for it a
        // while, then fails busy.
        conn.query("PRAGMA busy_timeout = 2000", ()).await
            .unwrap();
        conn
    }

    /// Keep `conn` open as a stream, and return the baton that names it.
    fn keep(&self, conn: Connection) -> String {
        let baton = format!("stream-{}", self.batons.fetch_add(1, Ordering::SeqCst));
        self.open
            .lock()
            .unwrap()
            .insert(baton.clone(), conn);
        baton
    }
}

/// A `/sync` that stops answering, as a primary on a broken network does.
/// While enabled, every `/sync` answers with the frames the primary had when
/// the first stalled request arrived, as a pull that started then sees them.
#[derive(Default)]
pub struct SyncStall {
    pub enabled: AtomicBool,
    /// Signalled when a `/sync` request starts stalling.
    pub started: tokio::sync::Notify,
    /// The generation and WAL the stalled pull answers with.
    frozen: std::sync::Mutex<Option<(usize, Vec<u8>)>>,
}

/// A primary at `path`, served on a loopback port.
///
/// # Panics
///
/// The database or the listener cannot be set up.
pub async fn serve_primary(
    path: PathBuf,
) -> (Arc<Primary>, SocketAddr, tokio::task::JoinHandle<()>) {
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
        streams: Streams {
            db,
            open: std::sync::Mutex::new(HashMap::new()),
            batons: AtomicUsize::new(0),
        },
        conn,
        path,
        exports: AtomicUsize::new(0),
        pipelines: AtomicUsize::new(0),
        round_trips: std::sync::Mutex::new(Vec::new()),
        generation: AtomicUsize::new(1),
        salt: std::sync::Mutex::new(None),
        sync_stall: SyncStall::default(),
    });
    let app = Router::new()
        .route("/info", get(info))
        .route("/export/{generation}", get(export))
        .route("/sync/{generation}/{start}/{end}", get(sync))
        .route("/v3/pipeline", post(pipeline))
        .route("/v3/cursor", post(cursor))
        .with_state(Arc::clone(&primary));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (primary, address, server)
}

async fn info(State(primary): State<Arc<Primary>>) -> Json<JsonValue> {
    let (generation, _) = primary.wal_state();
    Json(json!({ "current_generation": generation }))
}

async fn export(State(primary): State<Arc<Primary>>) -> Vec<u8> {
    // Another stream's open transaction locks the checkpoint out until it
    // ends.
    let mut attempts = 0;
    while let Err(error) = checkpoint(&primary.conn).await {
        attempts += 1;
        assert!(attempts < 500, "the primary's WAL never checkpointed: {error}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    primary.exports.fetch_add(1, Ordering::SeqCst);
    std::fs::read(&primary.path).unwrap()
}

/// Move the whole WAL into the database file, so the file is the export.
/// A checkpoint that a reader or writer blocked reports it in its `busy`
/// column rather than as an error, and leaves frames in the WAL.
async fn checkpoint(conn: &Connection) -> Result<(), String> {
    let mut rows = conn
        .query("PRAGMA wal_checkpoint(TRUNCATE)", ())
        .await
        .map_err(|error| error.to_string())?;
    let mut busy = false;
    while let Some(row) = rows.next().await.map_err(|error| error.to_string())? {
        busy |= row.get::<i64>(0).map_err(|error| error.to_string())? != 0;
    }
    if busy { Err("the checkpoint was busy".to_string()) } else { Ok(()) }
}

/// WAL frames `[start, end)` of `generation`, or the generation to move to.
async fn sync(
    State(primary): State<Arc<Primary>>,
    Path((generation, start, end)): Path<(usize, usize, usize)>,
) -> Result<Vec<u8>, (StatusCode, Json<JsonValue>)> {
    let (current, wal) = if primary.sync_stall.enabled.load(Ordering::SeqCst) {
        let (state, first) = {
            let mut frozen = primary.sync_stall.frozen.lock().unwrap();
            let first = frozen.is_none();
            (frozen.get_or_insert_with(|| primary.wal_state()).clone(), first)
        };
        if first {
            primary.sync_stall.started.notify_one();
            tokio::time::sleep(SYNC_STALL).await;
        }
        state
    } else {
        *primary.sync_stall.frozen.lock().unwrap() = None;
        primary.wal_state()
    };
    let frames = if generation == current { wal_frames(&wal, start, end) } else { None };
    frames.ok_or_else(|| (StatusCode::BAD_REQUEST, Json(json!({ "generation": current }))))
}

async fn pipeline(State(primary): State<Arc<Primary>>, body: String) -> Json<JsonValue> {
    let body: JsonValue = serde_json::from_str(&body).unwrap();
    let steps = body["requests"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|request| {
            request["batch"]["steps"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        });
    primary.record_round_trip(steps);
    let conn = primary.streams.take(body["baton"].as_str()).await;
    let mut results = Vec::new();
    let mut closed = false;
    for request in body["requests"].as_array().unwrap() {
        closed |= request["type"].as_str() == Some("close");
        results.push(Primary::answer(&conn, request).await);
    }
    let baton =
        if closed { JsonValue::Null } else { JsonValue::String(primary.streams.keep(conn)) };
    Json(json!({ "baton": baton, "base_url": null, "results": results }))
}

/// Run a batch and stream its results as Hrana cursor entries, one JSON
/// object per line.
async fn cursor(State(primary): State<Arc<Primary>>, body: String) -> String {
    let body: JsonValue = serde_json::from_str(&body).unwrap();
    primary.record_round_trip(
        body["batch"]["steps"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    let conn = primary.streams.take(body["baton"].as_str()).await;
    let entries = cursor_entries(&conn, body["batch"]["steps"].as_array().unwrap()).await;
    let baton = primary.streams.keep(conn);
    let mut lines = String::new();
    for line in std::iter::once(json!({ "baton": baton, "base_url": null })).chain(entries) {
        lines.push_str(&line.to_string());
        lines.push('\n');
    }
    lines
}

/// The cursor entries of running `steps`. A step after a failed one is
/// skipped.
async fn cursor_entries(conn: &Connection, steps: &[JsonValue]) -> Vec<JsonValue> {
    let mut entries = Vec::new();
    for (step, entry) in steps.iter().enumerate() {
        match Primary::run(conn, &entry["stmt"]).await {
            Ok(result) => {
                entries.push(json!({ "type": "step_begin", "step": step, "cols": result["cols"] }));
                for row in result["rows"].as_array().unwrap() {
                    entries.push(json!({ "type": "row", "row": row }));
                }
                entries.push(json!({
                    "type": "step_end",
                    "affected_row_count": result["affected_row_count"],
                    "last_insert_rowid": null,
                }));
            }
            Err(error) => {
                entries.push(
                    json!({ "type": "step_error", "step": step, "error": hrana_error(&error) }),
                );
                break;
            }
        }
    }
    entries
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
    /// The round trips to the primary so far.
    pub fn round_trip_count(&self) -> usize {
        self.pipelines.load(Ordering::SeqCst)
    }

    /// Take the statements of every round trip since the last call.
    pub fn take_round_trips(&self) -> Vec<Vec<String>> {
        std::mem::take(&mut *self.round_trips.lock().unwrap())
    }

    fn record_round_trip(&self, steps: impl IntoIterator<Item = JsonValue>) {
        let statements = steps
            .into_iter()
            .map(|step| {
                let sql = step["stmt"]["sql"].as_str().unwrap_or_default();
                let sql = sql
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                sql.chars().take(120).collect()
            })
            .collect();
        self.round_trips
            .lock()
            .unwrap()
            .push(statements);
        self.pipelines.fetch_add(1, Ordering::SeqCst);
    }

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

    async fn answer(conn: &Connection, request: &JsonValue) -> JsonValue {
        match request["type"].as_str() {
            Some("batch") => {
                Self::run_batch(conn, request["batch"]["steps"].as_array().unwrap()).await
            }
            Some("get_autocommit") => json!({
                "type": "ok",
                "response": {
                    "type": "get_autocommit",
                    "is_autocommit": conn.is_autocommit(),
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

    /// Run the steps of a batch as a Hrana server does: a step whose
    /// condition does not hold is skipped, with neither a result nor an
    /// error.
    async fn run_batch(conn: &Connection, steps: &[JsonValue]) -> JsonValue {
        let mut step_results = Vec::new();
        let mut step_errors = Vec::new();
        for step in steps {
            let holds = step
                .get("condition")
                .filter(|condition| !condition.is_null())
                .is_none_or(|condition| {
                    condition_holds(condition, &step_results, &step_errors, conn)
                });
            if !holds {
                step_results.push(JsonValue::Null);
                step_errors.push(JsonValue::Null);
                continue;
            }
            match Self::run(conn, &step["stmt"]).await {
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

    async fn run(conn: &Connection, stmt: &JsonValue) -> libsql::Result<JsonValue> {
        let sql = stmt["sql"].as_str().unwrap();
        let args: Vec<Value> = stmt["args"]
            .as_array()
            .map(|args| args.iter().map(hrana_arg).collect())
            .unwrap_or_default();
        let mut rows = conn.query(sql, args).await?;
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
        // A statement sent without `want_rows` gets no rows back.
        if stmt["want_rows"] == JsonValue::Bool(false) {
            encoded_rows.clear();
        }
        Ok(json!({
            "cols": cols,
            "rows": encoded_rows,
            "affected_row_count": conn.changes(),
            "last_insert_rowid": null,
        }))
    }
}

/// Whether a Hrana batch step's `condition` holds, given the results and
/// errors of the steps before it.
fn condition_holds(
    condition: &JsonValue,
    results: &[JsonValue],
    errors: &[JsonValue],
    conn: &Connection,
) -> bool {
    let step = || {
        condition["step"]
            .as_u64()
            .and_then(|step| usize::try_from(step).ok())
            .unwrap()
    };
    let each = |conds: &JsonValue| {
        conds
            .as_array()
            .unwrap()
            .iter()
            .map(|cond| condition_holds(cond, results, errors, conn))
            .collect::<Vec<_>>()
    };
    match condition["type"].as_str() {
        Some("ok") => results
            .get(step())
            .is_some_and(|result| !result.is_null()),
        Some("error") => errors
            .get(step())
            .is_some_and(|error| !error.is_null()),
        Some("not") => !condition_holds(&condition["cond"], results, errors, conn),
        Some("and") => each(&condition["conds"]).into_iter().all(|holds| holds),
        Some("or") => each(&condition["conds"]).into_iter().any(|holds| holds),
        Some("is_autocommit") => conn.is_autocommit(),
        other => panic!("unsupported Hrana batch condition {other:?}"),
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
        Some("blob") => Value::Blob(
            BASE64
                .decode(
                    arg["base64"]
                        .as_str()
                        .unwrap()
                        .trim_end_matches('='),
                )
                .unwrap(),
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
        Value::Blob(value) => json!({ "type": "blob", "base64": BASE64.encode(value) }),
    }
}

/// Turso reports a failed statement with the `SQLite` result code name.
fn hrana_error(error: &libsql::Error) -> JsonValue {
    let code = if is_unique_violation(error) {
        "SQLITE_CONSTRAINT"
    } else if is_transaction_conflict(error) {
        "SQLITE_BUSY"
    } else {
        "SQLITE_ERROR"
    };
    json!({ "message": error.to_string(), "code": code })
}
