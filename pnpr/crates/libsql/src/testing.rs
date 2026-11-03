//! A local primary that serves libSQL's newer sync protocol, as Turso Cloud
//! does. libsql discovers that protocol through the primary's `/info` route,
//! bootstraps an embedded replica from `/export`, pulls frames from `/sync`,
//! and sends writes to the primary over Hrana. The primary here is a local
//! libsql database behind a mock of those routes that serves the frames of
//! its own WAL.

pub use hrana::RoundTrips;
pub use plain_http::{EXECUTE_RESPONSE, PlainHttpServer, serve_plain_http};

mod hrana;
mod plain_http;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};
use libsql::{Builder, Connection, Database};
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
    /// The Hrana requests so far: the round trips to the primary.
    pub round_trips: RoundTrips,
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
        round_trips: RoundTrips::default(),
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
    primary.round_trips.record(steps);
    let conn = primary.streams.take(body["baton"].as_str()).await;
    let mut results = Vec::new();
    let mut closed = false;
    for request in body["requests"].as_array().unwrap() {
        closed |= request["type"].as_str() == Some("close");
        results.push(hrana::answer(&conn, request).await);
    }
    let baton =
        if closed { JsonValue::Null } else { JsonValue::String(primary.streams.keep(conn)) };
    Json(json!({ "baton": baton, "base_url": null, "results": results }))
}

/// Run a batch and stream its results as Hrana cursor entries, one JSON
/// object per line.
async fn cursor(State(primary): State<Arc<Primary>>, body: String) -> String {
    let body: JsonValue = serde_json::from_str(&body).unwrap();
    primary.round_trips.record(
        body["batch"]["steps"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    let conn = primary.streams.take(body["baton"].as_str()).await;
    let entries = hrana::cursor_entries(&conn, body["batch"]["steps"].as_array().unwrap()).await;
    let baton = primary.streams.keep(conn);
    let mut lines = String::new();
    for line in std::iter::once(json!({ "baton": baton, "base_url": null })).chain(entries) {
        lines.push_str(&line.to_string());
        lines.push('\n');
    }
    lines
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
}
