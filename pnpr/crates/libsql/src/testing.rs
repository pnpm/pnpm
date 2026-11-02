//! A local primary that serves libSQL's newer sync protocol, as Turso Cloud
//! does. libsql discovers that protocol through the primary's `/info` route,
//! bootstraps an embedded replica from `/export`, pulls frames from `/sync`,
//! and sends writes to the primary over Hrana. The primary here is a local
//! libsql database behind a mock of those routes that serves the frames of
//! its own WAL.

use crate::is_unique_violation;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use libsql::{Builder, Connection, Database, Value};
use serde_json::{Value as JsonValue, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver},
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
    _db: Database,
    /// The primary's own connection, which also runs every Hrana statement.
    pub conn: Connection,
    pub path: PathBuf,
    /// Exports so far; each one checkpoints the WAL.
    pub exports: AtomicUsize,
    /// Hrana pipeline requests so far: the statements sent to the primary.
    pub pipelines: AtomicUsize,
    /// The sync generation. `SQLite` restarts the WAL at frame 1 with a new
    /// salt after a checkpoint, which the protocol expresses as the next
    /// generation.
    generation: AtomicUsize,
    salt: std::sync::Mutex<Option<[u8; 8]>>,
    pub sync_stall: SyncStall,
}

/// A `/sync` that stops answering, as a primary on a broken network does.
#[derive(Default)]
pub struct SyncStall {
    pub enabled: AtomicBool,
    /// Signalled when a `/sync` request starts stalling.
    pub started: tokio::sync::Notify,
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

/// The Hrana pipeline response to libsql's request for one statement:
/// a batch with an empty step result, the autocommit state, and the close.
pub const EXECUTE_RESPONSE: &str = r#"{"baton":null,"base_url":null,"results":[{"type":"ok","response":{"type":"batch","result":{"step_results":[{"cols":[],"rows":[],"affected_row_count":0,"last_insert_rowid":null}],"step_errors":[null]}}},{"type":"ok","response":{"type":"get_autocommit","is_autocommit":true}},{"type":"ok","response":{"type":"close"}}]}"#;

/// A loopback plain-HTTP server that answers every request alike.
pub struct PlainHttpServer {
    pub url: String,
    /// The head of every request the server received, in order.
    pub requests: Receiver<String>,
}

/// Answer every request on a loopback plain-HTTP server with `body`.
#[must_use]
pub fn serve_plain_http(body: &'static str) -> PlainHttpServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (sender, requests) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = sender.send(answer(stream, body));
        }
    });
    PlainHttpServer { url, requests }
}

/// Answer one request with `body` and return the request's head.
fn answer(mut stream: TcpStream, body: &str) -> String {
    let mut reader = BufReader::new(&stream);
    let head = read_head(&mut reader);
    let content_length = head
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(str::to_owned)
        })
        .map(|value| value.trim().parse().unwrap())
        .unwrap_or(0);
    reader
        .read_exact(&mut vec![0; content_length])
        .unwrap();
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len(),
    );
    stream.write_all(response.as_bytes()).unwrap();
    head
}

fn read_head(reader: &mut impl BufRead) -> String {
    let mut head = String::new();
    while reader.read_line(&mut head).unwrap() > 2 {}
    head
}
