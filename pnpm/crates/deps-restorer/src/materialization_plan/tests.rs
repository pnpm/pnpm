use super::spawn_node_probe;
use std::{sync::mpsc, time::Duration};

#[test]
fn runtime_shutdown_does_not_wait_for_an_unread_node_probe() {
    let (release_probe, probe_released) = mpsc::channel::<()>();
    let (report_probe_start, probe_started) = mpsc::channel();
    let (report_shutdown, shutdown_reported) = mpsc::channel();
    std::thread::spawn(move || {
        let runtime =
            tokio::runtime::Builder::new_current_thread().build().expect("build the tokio runtime");
        let entered = runtime.enter();
        let probe = spawn_node_probe(move || {
            let _ = report_probe_start.send(());
            probe_released.recv()
        });
        probe_started
            .recv_timeout(Duration::from_secs(30))
            .expect("the probe starts");
        drop(probe);
        drop(entered);
        drop(runtime);
        let _ = report_shutdown.send(());
    });
    let shutdown = shutdown_reported.recv_timeout(Duration::from_secs(30));
    let _ = release_probe.send(());
    if let Err(error) = shutdown {
        panic!("the runtime owner did not report shutdown: {error:?}");
    }
}
