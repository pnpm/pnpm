use super::{Arc, Identity, Mutex, ResolveRequest, deps};
use crate::resolver::{
    Resolver, request_tarball_router,
    streaming::{StreamedResolveInputs, stream_resolve_response},
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
    runtime.configs.admission = Arc::new(tokio::sync::Semaphore::new(1));
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
    let response = stream_resolve_response(
        &runtime,
        StreamedResolveInputs {
            config: active,
            request,
            request_auth,
            tarball_router,
            footprint,
            cache_key: None,
        },
    );
    tokio::time::timeout(Duration::from_secs(5), request_started).await
        .expect("the resolution should reach the upstream")
        .unwrap();
    assert_eq!(runtime.configs.admission.available_permits(), 0);
    drop(response);
    let _permit = tokio::time::timeout(
        Duration::from_secs(5),
        Arc::clone(&runtime.configs.admission).acquire_owned(),
    )
    .await
    .expect("disconnecting should release admission without waiting for the upstream")
    .unwrap();
    upstream.abort();
    let error = upstream.await.unwrap_err();
    assert!(error.is_cancelled(), "{error}");
}
