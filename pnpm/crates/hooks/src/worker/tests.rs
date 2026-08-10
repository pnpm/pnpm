use std::sync::Arc;

use tempfile::TempDir;
use tokio::{
    task::JoinSet,
    time::{Duration, timeout},
};

use super::{MAX_IN_FLIGHT_REQUESTS, NodeWorker};

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

#[tokio::test]
async fn a_fan_out_of_requests_reaches_the_worker_at_the_in_flight_cap() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(
        &pnpmfile_path,
        r"let inFlight = 0
let peak = 0
module.exports = {
  hooks: {
    readPackage: async (pkg) => {
      inFlight++
      peak = Math.max(peak, inFlight)
      await new Promise((resolve) => setTimeout(resolve, 100))
      inFlight--
      pkg.peak = peak
      return pkg
    },
  },
}
",
    )
    .expect("write pnpmfile");
    let worker = NodeWorker::spawn(&pnpmfile_path).await.expect("spawn worker");

    let peak = call_read_package_concurrently(&worker, 100).await;
    assert_eq!(
        peak, MAX_IN_FLIGHT_REQUESTS,
        "every slot should be used, and no request should reach the worker beyond them",
    );
}

#[tokio::test]
async fn a_queued_request_does_not_spend_its_timeout_waiting_for_its_turn() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    // The hook blocks Node's event loop, so the worker services the fan-out
    // strictly one request at a time: 100 of them outlast the timeout below
    // several times over, and only a request whose window starts at its turn
    // can survive.
    std::fs::write(
        &pnpmfile_path,
        r"module.exports = {
  hooks: {
    readPackage: (pkg) => {
      const until = Date.now() + 30
      while (Date.now() < until) {}
      return pkg
    },
  },
}
",
    )
    .expect("write pnpmfile");
    let worker = NodeWorker::spawn_with_request_timeout(&pnpmfile_path, Duration::from_secs(2))
        .await
        .expect("spawn worker");

    call_read_package_concurrently(&worker, 100).await;
}

/// Every call must succeed; the value returned is the highest `peak` the
/// hook reported back.
async fn call_read_package_concurrently(worker: &Arc<NodeWorker>, count: usize) -> usize {
    let mut calls = JoinSet::new();
    for _ in 0..count {
        let worker = Arc::clone(worker);
        calls.spawn(async move {
            worker.call("readPackage", serde_json::json!({}), Arc::new(|_| {})).await
        });
    }

    let mut peak = 0;
    while let Some(call) = calls.join_next().await {
        let result = call.expect("join the call").expect("the hook should not fail");
        peak = peak.max(result["peak"].as_u64().unwrap_or(0) as usize);
    }
    peak
}
