use std::sync::Arc;

use tempfile::TempDir;
use tokio::time::{Duration, timeout};

use super::NodeWorker;

#[tokio::test]
async fn cancelled_request_removes_its_pending_entry() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(
        &pnpmfile_path,
        "module.exports = { hooks: { readPackage: () => new Promise(() => {}) } }\n",
    )
    .expect("write pnpmfile");
    let worker = NodeWorker::spawn(&pnpmfile_path).await.expect("spawn worker");

    let call = worker.call("readPackage", serde_json::json!({}), Arc::new(|_| {}));
    let cancelled = timeout(Duration::from_millis(500), call).await;
    assert!(cancelled.is_err(), "the never-resolving hook must outlive the local timeout");

    assert!(
        worker.pending.lock().unwrap().is_empty(),
        "a cancelled request must not leak its pending entry",
    );
}
