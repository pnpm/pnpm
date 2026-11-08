use super::{AsyncCommand, Command};
use std::{
    fs,
    process::Stdio,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[test]
fn output_captures_stdout_and_stderr() {
    let _test_lock = crate::tests::test_lock();
    let output = Command::new("sh")
        .args(["-c", "printf stdout; printf stderr >&2"])
        .output()
        .expect("run shell");

    assert_eq!(output.stdout, b"stdout");
    assert_eq!(output.stderr, b"stderr");
}

#[test]
fn output_preserves_configured_stdio() {
    let _test_lock = crate::tests::test_lock();
    let output = Command::new("sh")
        .args(["-c", "printf stdout"])
        .stdout(Stdio::null())
        .output()
        .expect("run shell");

    assert!(output.stdout.is_empty());
}

#[test]
fn spawn_waits_until_watchdog_has_started() {
    let _test_lock = crate::tests::test_lock();
    let watchdog = crate::watchdog_spawning();
    let (ready, started) = mpsc::channel();
    let (spawned, child_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 30"]);
        ready
            .send(())
            .expect("signal before spawn");
        spawned.send(command.spawn()).expect("send spawn result");
    });

    started.recv().expect("spawn thread is ready");
    assert!(
        child_rx
            .recv_timeout(Duration::from_millis(50))
            .is_err(),
    );
    drop(watchdog);
    let mut child = child_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("spawn proceeds")
        .expect("spawn shell");
    worker.join().expect("spawn thread completes");
    child.kill().expect("stop shell");
    child.wait().expect("reap shell");
}

#[tokio::test(flavor = "multi_thread")]
async fn asynchronous_output_captures_stdout_and_stderr() {
    let _test_lock = crate::tests::test_lock_async().await;
    let output = AsyncCommand::new("sh")
        .args(["-c", "printf stdout; printf stderr >&2"])
        .output()
        .await
        .expect("run shell");

    assert_eq!(output.stdout, b"stdout");
    assert_eq!(output.stderr, b"stderr");
}

#[tokio::test(flavor = "multi_thread")]
async fn asynchronous_output_preserves_configured_stdio() {
    let _test_lock = crate::tests::test_lock_async().await;
    let output = AsyncCommand::new("sh")
        .args(["-c", "printf stdout; printf stderr >&2"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await
        .expect("run shell");

    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn asynchronous_spawn_waits_until_watchdog_has_started() {
    let _test_lock = crate::tests::test_lock_async().await;
    let watchdog = crate::watchdog_spawning();
    let (ready, started) = mpsc::channel();
    let (spawned, child_rx) = mpsc::channel();
    let worker = tokio::task::spawn_blocking(move || {
        let mut command = AsyncCommand::new("sh");
        command.args(["-c", "sleep 30"]);
        ready
            .send(())
            .expect("signal before spawn");
        spawned.send(command.spawn()).expect("send spawn result");
    });

    started.recv().expect("spawn thread is ready");
    assert!(
        child_rx
            .recv_timeout(Duration::from_millis(50))
            .is_err(),
    );
    drop(watchdog);
    let mut child = child_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("spawn proceeds")
        .expect("spawn shell");
    worker.await.expect("spawn thread completes");
    child.kill().await.expect("stop shell");
    child.wait().await.expect("reap shell");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn asynchronous_status_releases_spawn_lock_while_waiting() {
    let _test_lock = crate::tests::test_lock_async().await;
    let marker = std::env::temp_dir()
        .join(format!(
            "pnpm-process-{}-{}.txt",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time is after the Unix epoch")
                .as_nanos(),
        ));
    let marker_for_command = marker.to_string_lossy().into_owned();
    let status_task = tokio::spawn(async move {
        let mut command = AsyncCommand::new("sh");
        command.args(["-c", r#"printf ready > "$1"; sleep 1"#, "sh", &marker_for_command]);
        command.status().await
    });
    let started = Instant::now();
    while !marker.exists() && started.elapsed() < Duration::from_secs(1) {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(marker.exists(), "child must start before lock acquisition");

    let (acquired, acquired_rx) = mpsc::channel();
    let watchdog_task = tokio::task::spawn_blocking(move || {
        let watchdog = crate::watchdog_spawning();
        acquired
            .send(())
            .expect("signal watchdog lock acquired");
        drop(watchdog);
    });
    acquired_rx
        .recv_timeout(Duration::from_millis(250))
        .expect("watchdog lock is available while the child runs");
    watchdog_task.await.expect("watchdog thread completes");
    assert!(
        status_task.await
            .expect("status task completes")
            .expect("shell exits")
            .success(),
    );
    fs::remove_file(marker).expect("remove child marker");
}
