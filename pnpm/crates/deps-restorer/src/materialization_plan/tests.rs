use super::{
    HostNode,
    TargetNodeUse,
    execution_host_node,
    spawn_node_probe,
    target_node_major,
    target_node_version,
};
use crate::{
    InstallabilityHost,
    NodeVersionSource,
};
use std::{
    sync::mpsc,
    time::Duration,
};

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

#[test]
fn compatibility_prefers_node_version_then_runtime_pin_then_host() {
    let compatibility =
        |config, pin, host| target_node_version(TargetNodeUse::Compatibility, config, pin, host);
    assert_eq!(compatibility(Some("20.0.0"), Some("24.0.0"), Some("22.0.0")), Some("20.0.0"));
    assert_eq!(compatibility(None, Some("24.0.0"), Some("22.0.0")), Some("24.0.0"));
    assert_eq!(compatibility(None, None, Some("22.0.0")), Some("22.0.0"));
}

#[test]
fn execution_ignores_node_version() {
    let execution =
        |config, pin, host| target_node_major(TargetNodeUse::Execution, config, pin, host);
    assert_eq!(execution(Some("20.0.0"), Some("24.0.0"), Some("22.0.0")), Some(24));
    assert_eq!(execution(Some("20.0.0"), None, Some("22.0.0")), Some(22));
    assert_eq!(execution(Some("20.0.0"), None, None), None);
}

#[test]
fn execution_host_node_keeps_only_a_probed_host() {
    let overridden = InstallabilityHost::detect_with(false, Some("20.0.0".to_owned()));
    assert!(execution_host_node(Some(&overridden)).is_none());
    let probed = InstallabilityHost { node_source: NodeVersionSource::Probed, ..overridden };
    let HostNode { version, detected } =
        execution_host_node(Some(&probed)).expect("a probed host keys the store");
    assert_eq!((version.as_str(), detected), ("20.0.0", true));
}
