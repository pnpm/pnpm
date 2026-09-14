use super::{MockInstance, MockInstanceOptions};
use crate::{pick_port::pick_unused_port, process_kill::kill_process_by_pid};
use reqwest::{Client, Proxy};
use std::{ffi::OsStr, process};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, Signal, System, UpdateKind};
use tokio::{
    net::{TcpListener, TcpStream},
    task::JoinHandle,
    time::{Duration, timeout},
};

fn options(client: &Client, port: u16) -> MockInstanceOptions<'_> {
    MockInstanceOptions {
        client,
        port,
        public_url: None,
        stdout: None,
        stderr: None,
        max_retries: 20,
        retry_delay: Duration::from_millis(500),
    }
}

fn processes() -> System {
    System::new_with_specifics(
        RefreshKind::nothing()
            .with_processes(ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always)),
    )
}

/// Last-resort kill for the registry child a test spawned, so a failing
/// assertion never leaves the process behind.
struct ChildCleanup(Option<Pid>);

impl ChildCleanup {
    /// Disarms once the child is confirmed gone, because the OS is free to
    /// hand that PID to an unrelated process afterwards.
    fn assert_reaped(mut self) {
        let pid = self.0.expect("cleanup is armed");
        assert!(
            processes().process(pid).is_none(),
            "registry child {pid} must be stopped and reaped",
        );
        self.0 = None;
    }
}

impl Drop for ChildCleanup {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            let _ = kill_process_by_pid(pid, Signal::Kill);
        }
    }
}

/// Drive [`MockInstanceOptions::spawn`] up to its first readiness request,
/// which a proxy that never answers parks indefinitely. The returned task is
/// therefore suspended with a live registry child, which is the state both
/// cleanup paths have to recover from.
async fn pending_startup() -> (JoinHandle<MockInstance>, TcpStream, ChildCleanup) {
    let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Client::builder()
        .proxy(Proxy::http(format!("http://{}", proxy.local_addr().unwrap())).unwrap())
        .build()
        .unwrap();
    let port = pick_unused_port().unwrap();
    let startup = tokio::spawn(async move { options(&client, port).spawn().await });
    let (connection, _) = timeout(Duration::from_secs(10), proxy.accept()).await
        .expect("startup must reach its readiness request")
        .unwrap();
    let system = processes();
    let address = format!("127.0.0.1:{port}");
    let child = system
        .processes()
        .values()
        .find(|child| {
            child.parent() == Some(Pid::from_u32(process::id()))
                && child
                    .cmd()
                    .iter()
                    .any(|arg| arg == OsStr::new(&address))
        })
        .expect("startup must have a live registry child");
    (startup, connection, ChildCleanup(Some(child.pid())))
}

#[tokio::test]
async fn cancelled_startup_reaps_child() {
    let (startup, _connection, child) = pending_startup().await;
    startup.abort();
    let error = startup.await.unwrap_err();
    assert!(error.is_cancelled(), "aborting startup cancels it: {error}");
    child.assert_reaped();
}

#[tokio::test]
async fn failed_readiness_check_reaps_child() {
    let (startup, connection, child) = pending_startup().await;
    drop(connection);
    // Generous because a platform that reports the closed connection as a
    // connect error sends startup around its whole retry budget first.
    let error = timeout(Duration::from_secs(30), startup).await
        .expect("closed connection must fail the readiness check")
        .unwrap_err();
    assert!(error.is_panic(), "readiness failures panic: {error}");
    child.assert_reaped();
}

#[tokio::test]
async fn successful_startup_retains_owner_and_reuse_does_not_stop_registry() {
    let client = Client::builder()
        .no_proxy()
        .build()
        .unwrap();
    let registry = options(&client, pick_unused_port().unwrap());
    let instance = registry.spawn().await;
    let child = ChildCleanup(Some(Pid::from_u32(instance.process.id())));
    assert!(registry.spawn_if_necessary().await.is_none(), "a ready registry is not respawned");
    assert!(registry.is_registry_ready().await, "reuse must leave the registry serving");
    drop(instance);
    child.assert_reaped();
}
