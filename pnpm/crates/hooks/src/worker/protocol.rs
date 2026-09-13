use super::PendingMap;
use crate::FetcherCallback;
use serde_json::Value;
use std::sync::Arc;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout},
    sync::{Mutex, oneshot},
};

/// Route one line from the worker to its pending request: forward `log` lines
/// to the call's logger (the entry stays until the result arrives) and resolve
/// the call on `ok`/`err`.
/// Dispatch every line the worker writes; once it exits, fail every
/// still-pending request so callers don't hang waiting for a response
/// that will never arrive.
pub(super) fn spawn_stdout_reader(
    stdout: ChildStdout,
    pending: PendingMap,
    stdin: Arc<Mutex<ChildStdin>>,
) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            dispatch_line(&pending, &stdin, &line);
        }
        for (_, request) in pending
            .lock()
            .unwrap()
            .drain()
        {
            let _ = request.done.send(Err("pnpmfile worker exited".to_string()));
        }
    });
}

fn dispatch_line(pending: &PendingMap, stdin: &Arc<Mutex<ChildStdin>>, line: &str) {
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return;
    };
    let Some(id) = message.get("id").and_then(Value::as_u64) else {
        return;
    };

    if let Some(log) = message.get("log").and_then(Value::as_str) {
        dispatch_log(pending, id, log);
        return;
    }

    if let Some(callback) = message.get("callback") {
        dispatch_callback(pending, stdin, id, callback);
        return;
    }

    let Some(entry) = pending
        .lock()
        .unwrap()
        .remove(&id)
    else {
        return;
    };
    let result = match message.get("err").and_then(Value::as_str) {
        Some(err) => Err(err.to_string()),
        None => Ok(message
            .get("ok")
            .cloned()
            .unwrap_or(Value::Null)),
    };
    let _ = entry.done.send(result);
}

/// Hand one fetcher callback to the request that opened it, and reply to the
/// worker with whatever it answers.
fn dispatch_callback(
    pending: &PendingMap,
    stdin: &Arc<Mutex<ChildStdin>>,
    id: u64,
    callback: &Value,
) {
    let Some(callback_id) = callback.get("id").and_then(Value::as_u64) else {
        return;
    };
    let Some(method) = callback.get("method").cloned() else {
        return;
    };
    let Ok(method) = serde_json::from_value(method) else {
        return;
    };
    let resolution = callback
        .get("resolution")
        .cloned()
        .unwrap_or(Value::Null);
    let options = callback
        .get("options")
        .cloned()
        .unwrap_or(Value::Null);
    let callbacks = pending
        .lock()
        .unwrap()
        .get(&id)
        .and_then(|entry| entry.callbacks.clone());
    let (response, receiver) = oneshot::channel();
    if let Some(callbacks) = callbacks {
        let _ = callbacks.send(FetcherCallback {
            method,
            resolution,
            options,
            response,
        });
    }
    reply_when_answered(Arc::clone(stdin), callback_id, receiver);
}

fn reply_when_answered(
    stdin: Arc<Mutex<ChildStdin>>,
    callback_id: u64,
    receiver: oneshot::Receiver<Result<Value, Value>>,
) {
    tokio::spawn(async move {
        let result = receiver.await.unwrap_or_else(|_| {
            Err(serde_json::json!({
                "message": "built-in fetcher callback is unavailable",
                "code": "ERR_PNPM_FETCHER_CALLBACK_UNAVAILABLE",
            }))
        });
        let reply = match result {
            Ok(value) => serde_json::json!({ "callbackResponse": callback_id, "ok": value }),
            Err(error) => serde_json::json!({ "callbackResponse": callback_id, "err": error }),
        };
        let _ = write_worker_line(&stdin, &reply).await;
    });
}

/// Write and flush one JSON line to the worker's stdin.
pub(super) async fn write_worker_line(
    stdin: &Mutex<ChildStdin>,
    reply: &Value,
) -> std::io::Result<()> {
    let mut line = serde_json::to_string(reply)?;
    line.push('\n');
    let mut stdin = stdin.lock().await;
    stdin.write_all(line.as_bytes()).await?;
    stdin.flush().await
}

fn dispatch_log(pending: &PendingMap, id: u64, log: &str) {
    let log_fn = pending
        .lock()
        .unwrap()
        .get(&id)
        .map(|entry| Arc::clone(&entry.log));
    if let Some(log_fn) = log_fn {
        log_fn(log.to_string());
    }
}
