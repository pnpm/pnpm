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
        worker.pending
            .lock()
            .unwrap()
            .is_empty(),
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

fn recording_logger(
    messages: &Arc<std::sync::Mutex<Vec<String>>>,
) -> crate::PreResolutionHookLogger {
    let info = Arc::clone(messages);
    let warn = Arc::clone(messages);
    crate::PreResolutionHookLogger {
        info: Arc::new(move |message| info.lock().unwrap().push(message)),
        warn: Arc::new(move |message| warn.lock().unwrap().push(message)),
    }
}

#[tokio::test]
async fn pre_resolution_is_not_bound_by_the_request_timeout() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(
        &pnpmfile_path,
        "module.exports = { hooks: { preResolution: () => new Promise((resolve) => setTimeout(resolve, 1500)) } }\n",
    )
    .expect("write pnpmfile");
    let worker = NodeWorker::spawn_with_request_timeout(&pnpmfile_path, Duration::from_secs(1))
        .await
        .expect("spawn worker");

    let messages = Arc::default();
    worker
        .call_pre_resolution(serde_json::json!({}), recording_logger(&messages))
        .await
        .expect("a slow preResolution hook completes");
}

/// Output a hook prints goes to the log of the call that printed it, even
/// while another call on the same worker is running.
#[tokio::test]
async fn concurrent_pre_resolution_calls_keep_their_own_output() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(
        &pnpmfile_path,
        r"module.exports = { hooks: { preResolution: async (ctx) => {
  console.log(ctx.name + ' before');
  await new Promise((resolve) => setTimeout(resolve, ctx.delay));
  console.log(ctx.name + ' after');
} } }
",
    )
    .expect("write pnpmfile");
    let worker = NodeWorker::spawn(&pnpmfile_path).await.expect("spawn worker");

    let slow_messages = Arc::default();
    let fast_messages = Arc::default();
    let slow = worker.call_pre_resolution(
        serde_json::json!({ "name": "slow", "delay": 300 }),
        recording_logger(&slow_messages),
    );
    let fast = worker.call_pre_resolution(
        serde_json::json!({ "name": "fast", "delay": 50 }),
        recording_logger(&fast_messages),
    );
    let (slow, fast) = tokio::join!(slow, fast);
    slow.expect("slow call");
    fast.expect("fast call");

    assert_eq!(*slow_messages.lock().unwrap(), ["slow before", "slow after"]);
    assert_eq!(*fast_messages.lock().unwrap(), ["fast before", "fast after"]);
}

/// A range that is not a string makes the manifest invalid, and the worker
/// sends the manifest back as JSON, which drops the entry. The call fails
/// instead, naming the dependency, the field and the package.
#[tokio::test]
async fn a_non_string_range_from_read_package_fails_the_call() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(
        &pnpmfile_path,
        r"module.exports = {
  hooks: {
    readPackage: (pkg) => {
      pkg.dependencies['ms'] = undefined
      return pkg
    },
  },
}
",
    )
    .expect("write pnpmfile");
    let worker = NodeWorker::spawn(&pnpmfile_path).await.expect("spawn worker");

    let err = worker
        .call(
            "readPackage",
            serde_json::json!({ "name": "debug", "version": "4.3.4" }),
            Arc::new(|_| {}),
        )
        .await
        .expect_err("a range that is not a string must fail the call");
    assert!(
        err.to_string().contains(
            "readPackage hook returned an invalid range for 'ms' in the 'dependencies' of debug@4.3.4. Expected a string, got undefined."
        ),
        "{err}",
    );
}

/// A pipe end pnpm leaves inheritable while the worker spawns must not stay
/// open in the worker, or reading the pipe to EOF waits until pnpm exits.
#[cfg(target_vendor = "apple")]
#[tokio::test]
async fn the_worker_does_not_hold_an_inheritable_pipe_open() {
    use std::{io::Read, os::fd::AsFd as _};

    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(&pnpmfile_path, "module.exports = { hooks: {} }\n").expect("write pnpmfile");
    let (mut read_end, write_end) = std::io::pipe().expect("create the pipe");
    assert!(
        pnpm_ffi_process::allow_inheritance(write_end.as_fd()),
        "make the write end inheritable",
    );

    let worker = NodeWorker::spawn(&pnpmfile_path).await.expect("spawn worker");
    assert!(worker.has_hooks().await, "the worker should be running");
    drop(write_end);

    let read_to_eof = tokio::task::spawn_blocking(move || read_end.read_to_end(&mut Vec::new()));
    timeout(Duration::from_secs(10), read_to_eof).await
        .expect("the worker must not hold the write end open")
        .expect("join the read")
        .expect("read the pipe");
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
