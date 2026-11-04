//! How the mock primary runs a Hrana request: the steps of a batch, with
//! their conditions, and the values and errors they carry.

use crate::{is_transaction_conflict, is_unique_violation};
use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD as BASE64};
use libsql::{Connection, Value};
use serde_json::{Value as JsonValue, json};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

/// The Hrana pipeline and cursor requests a primary has answered: the round
/// trips a client made to it.
#[derive(Default)]
pub struct RoundTrips {
    count: AtomicUsize,
    /// The statements of each request, in order, for a test to report what a
    /// round trip carried.
    statements: Mutex<Vec<Vec<String>>>,
}

impl RoundTrips {
    /// The round trips so far.
    pub fn count(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }

    /// Take the statements of every round trip since the last call, each
    /// statement's whitespace collapsed and cut at 120 characters.
    pub fn take(&self) -> Vec<Vec<String>> {
        std::mem::take(&mut *self.statements.lock().unwrap())
    }

    pub(super) fn record(&self, steps: impl IntoIterator<Item = JsonValue>) {
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
        self.statements
            .lock()
            .unwrap()
            .push(statements);
        self.count.fetch_add(1, Ordering::SeqCst);
    }
}

pub(super) async fn answer(conn: &Connection, request: &JsonValue) -> JsonValue {
    match request["type"].as_str() {
        Some("batch") => run_batch(conn, request["batch"]["steps"].as_array().unwrap()).await,
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
            .is_none_or(|condition| condition_holds(condition, &step_results, &step_errors, conn));
        if !holds {
            step_results.push(JsonValue::Null);
            step_errors.push(JsonValue::Null);
            continue;
        }
        match run(conn, &step["stmt"]).await {
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

/// The cursor entries of running `steps`. A step after a failed one is
/// skipped.
pub(super) async fn cursor_entries(conn: &Connection, steps: &[JsonValue]) -> Vec<JsonValue> {
    let mut entries = Vec::new();
    for (step, entry) in steps.iter().enumerate() {
        match run(conn, &entry["stmt"]).await {
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
