use super::{Arc, Identity, Mutex, ResolveRequest, deps};
use crate::resolver::{
    Resolver, request_tarball_router,
    streaming::{StreamedResolveInputs, send_keepalives, stream_resolve_response},
};
use pnpr_config::Config;
use pnpr_fixtures::MockConfigPresets;
use pnpr_route::Footprint;
use std::time::Duration;
use tokio::{io::AsyncReadExt as _, net::TcpListener, sync::oneshot};

#[tokio::test]
async fn dropping_a_stream_cancels_pending_resolution_and_releases_admission() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let registry = format!("http://{}/", listener.local_addr().unwrap());
    let (started, request_started) = oneshot::channel();
    let upstream = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 1024];
        assert_ne!(stream.read(&mut buffer).await.unwrap(), 0);
        started.send(()).unwrap();
        std::future::pending::<()>().await;
    });
    let temp = tempfile::tempdir().unwrap();
    let mut config = Config::proxy("127.0.0.1:7677".parse().unwrap(), temp.path().to_owned());
    config.http.public_url = "http://example.test".into();
    config.routing.upstreams
        .get_mut("npmjs")
        .unwrap()
        .url
        .clone_from(&registry);
    config.routing.route_policy.allowed_private_networks =
        vec![pnpr_config::IpNetwork::parse("127.0.0.1").unwrap()];
    let mut runtime = Resolver::build(&config, None);
    runtime.cache.configs.admission = Arc::new(tokio::sync::Semaphore::new(1));
    let request = ResolveRequest {
        registry: Some(registry),
        dependencies: Some(deps(&[("example-package", "1.0.0")])),
        ..Default::default()
    };
    let active = runtime
        .config_for(&request)
        .ok()
        .unwrap();
    let footprint = Arc::new(Mutex::new(Footprint::default()));
    let identity = Identity::user("client");
    let request_auth = runtime.hooked_auth(&request, &identity, &footprint);
    let tarball_router = request_tarball_router(&runtime, &identity, &active);
    let deadline = runtime.limits.start_deadline();
    let permit = runtime.limits
        .admit(&identity)
        .ok()
        .unwrap()
        .acquire(deadline)
        .await
        .ok()
        .unwrap();
    let response = stream_resolve_response(
        &runtime,
        StreamedResolveInputs {
            config: active,
            request,
            request_auth,
            tarball_router,
            footprint,
            cache_key: None,
            permit: Arc::new(permit),
            deadline,
        },
    );
    tokio::time::timeout(Duration::from_secs(5), request_started).await
        .expect("the resolution should reach the upstream")
        .unwrap();
    assert_eq!(runtime.cache.configs.admission.available_permits(), 0);
    drop(response);
    let _permit = tokio::time::timeout(
        Duration::from_secs(5),
        Arc::clone(&runtime.cache.configs.admission).acquire_owned(),
    )
    .await
    .expect("disconnecting should release admission without waiting for the upstream")
    .unwrap();
    upstream.abort();
    let error = upstream.await.unwrap_err();
    assert!(error.is_cancelled(), "{error}");
}

#[tokio::test(start_paused = true)]
async fn keepalives_send_progress_frames_until_the_client_hangs_up() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(crate::resolver::limits::STREAM_CHANNEL_CAPACITY);
    let keepalives = tokio::spawn(async move {
        send_keepalives(&tx, Duration::from_secs(15)).await;
    });
    for _ in 0..2 {
        let frame = rx.recv().await.expect("progress frame");
        let frame: serde_json::Value = serde_json::from_slice(&frame).unwrap();
        assert_eq!(frame, serde_json::json!({ "type": "progress" }));
    }
    drop(rx);
    tokio::time::timeout(Duration::from_secs(60), keepalives).await
        .expect("keepalives stop once the client hangs up")
        .unwrap();
}
