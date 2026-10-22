//! An embedded replica against a primary that serves libsql's newer sync
//! protocol, as Turso Cloud does. libsql discovers that protocol through
//! the primary's `/info` route, bootstraps the replica from `/export`,
//! pulls frames from `/sync`, and sends writes to the primary over Hrana.
//! The primary here is a local libsql database behind a mock of those
//! routes.

use super::{Builder, LibsqlAuth, MaxUsers, TokenBackend, UserBackend, is_unique_violation};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use libsql::Value;
use pnpr_config::LibsqlSettings;
use serde_json::{Value as JsonValue, json};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Primary {
    backend: LibsqlAuth,
    path: PathBuf,
    exports: AtomicUsize,
    pipelines: AtomicUsize,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn embedded_replica_follows_a_primary_on_the_newer_sync_protocol() {
    let directory = tempfile::tempdir().unwrap();
    let primary_path = directory.path().join("primary.db");
    let db = Builder::new_local(&primary_path).build().await.unwrap();
    let backend = LibsqlAuth::from_database(db, MaxUsers::Unlimited).await.unwrap();
    backend.add_or_login("alice", "secret").await.unwrap();
    let token = backend.issue("alice").await.unwrap();
    let primary = Arc::new(Primary {
        backend,
        path: primary_path,
        exports: AtomicUsize::new(0),
        pipelines: AtomicUsize::new(0),
    });
    let app = Router::new()
        .route("/info", get(info))
        .route("/export/{generation}", get(export))
        .route("/sync/{generation}/{start}/{end}", get(end_of_generation))
        .route("/v3/pipeline", post(pipeline))
        .with_state(Arc::clone(&primary));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    for (index, sync_interval_secs) in [None, Some(0)].into_iter().enumerate() {
        let settings = LibsqlSettings {
            url: format!("http://{address}"),
            auth_token: None,
            replica_path: Some(
                directory
                    .path()
                    .join(format!("replica{index}"))
                    .join("auth.db"),
            ),
            sync_interval_secs,
        };
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
        let owner = primary.backend.lookup(&issued).await.unwrap();
        assert_eq!(owner.as_deref(), Some("alice"));
    }
    server.abort();
}

async fn info() -> Json<JsonValue> {
    Json(json!({ "current_generation": 1 }))
}

async fn export(State(primary): State<Arc<Primary>>) -> Vec<u8> {
    primary.exports.fetch_add(1, Ordering::SeqCst);
    let mut checkpoint = primary.backend.conn
        .query("PRAGMA wal_checkpoint(TRUNCATE)", ())
        .await
        .unwrap();
    while checkpoint
        .next()
        .await
        .unwrap()
        .is_some()
    {}
    std::fs::read(&primary.path).unwrap()
}

/// The primary has no frames beyond the export, so every pull ends the
/// only generation.
async fn end_of_generation() -> (StatusCode, Json<JsonValue>) {
    (StatusCode::BAD_REQUEST, Json(json!({ "generation": 1 })))
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

impl Primary {
    async fn answer(&self, request: &JsonValue) -> JsonValue {
        match request["type"].as_str() {
            Some("batch") => self.run_batch(request["batch"]["steps"].as_array().unwrap()).await,
            Some("get_autocommit") => json!({
                "type": "ok",
                "response": {
                    "type": "get_autocommit",
                    "is_autocommit": self.backend.conn.is_autocommit(),
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
        let mut rows = self.backend.conn.query(sql, args).await?;
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
            "affected_row_count": self.backend.conn.changes(),
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
