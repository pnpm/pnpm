use std::sync::Arc;

use axum::body::{
    Bytes,
    to_bytes,
};
use pnpr_config::{
    IpNetwork,
    PublicRoute,
};
use serde_json::json;
use tokio::{
    io::{
        AsyncReadExt as _,
        AsyncWriteExt as _,
    },
    net::TcpListener,
};

use super::{
    StatusCode,
    config_for_registry,
    lockfile,
    registry_config,
    tarball_router,
    upstream_with_access,
    user,
};
use crate::{
    resolver::{
        Resolver,
        dispatch::handle_resolve,
    },
    server::ServerMetrics,
};

/// Serve `public-dep@1.0.0` to every request, closing each connection.
async fn serve_public_packument() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let tarball_origin = origin.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0_u8; 512];
            let _ = socket.read(&mut buf).await;
            let packument = json!({
                "name": "public-dep",
                "dist-tags": { "latest": "1.0.0" },
                "versions": {
                    "1.0.0": {
                        "name": "public-dep",
                        "version": "1.0.0",
                        "dist": {
                            "tarball": format!("{tarball_origin}/public-dep-1.0.0.tgz"),
                            "integrity": ssri::Integrity::from(b"pkg-bytes").to_string(),
                        },
                    },
                },
            })
            .to_string();
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{packument}",
                packument.len(),
            );
            let _ = socket.write_all(reply.as_bytes()).await;
        }
    });
    (origin, server)
}

async fn resolve_to_done(resolver: &Resolver, body: &str) {
    let response = handle_resolve(resolver, user("alice"), Bytes::from(body.to_owned())).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body =
        String::from_utf8(to_bytes(response.into_body(), usize::MAX).await.unwrap().to_vec())
            .unwrap();
    assert!(body.contains(r#""type":"done""#), "{body}");
}

#[tokio::test]
async fn resolution_cache_metrics_split_public_hits_misses_and_stores() {
    let (origin, server) = serve_public_packument().await;
    let dir = tempfile::tempdir().unwrap();
    let mut config = registry_config();
    config.storage.cache_dir = dir.path().to_path_buf();
    config.http.public_url = "http://example.test".into();
    config.routing.route_policy.public.push(PublicRoute {
        registry: Some(format!("{origin}/")),
        package: None,
    });
    config.routing.route_policy.allowed_private_networks =
        vec![IpNetwork::parse("127.0.0.0/8").unwrap()];
    let resolver = Resolver::build(&config, None);
    let body = json!({
        "registry": format!("{origin}/"),
        "dependencies": { "public-dep": "^1.0.0" },
        "trustPolicy": "off",
        "minimumReleaseAge": 0,
    })
    .to_string();

    resolve_to_done(&resolver, &body).await;
    let cold = resolver.cache.metrics.render_prometheus();
    assert!(cold.contains("pnpr_resolution_cache_misses_total 1"), "{cold}");
    assert!(cold.contains(r#"pnpr_resolution_cache_stores_total{footprint="public"} 1"#));
    assert!(cold.contains(r#"pnpr_resolution_cache_stores_total{footprint="private"} 0"#));
    assert!(cold.contains(r#"pnpr_resolution_cache_hits_total{footprint="public"} 0"#));
    assert!(!cold.contains(r#"pnpr_tarball_routes_total{route="upstream"} 0"#), "{cold}");
    assert!(cold.contains(r#"pnpr_tarball_routes_total{route="gateway"} 0"#));

    resolve_to_done(&resolver, &body).await;
    let warm = resolver.cache.metrics.render_prometheus();
    assert!(warm.contains("pnpr_resolution_cache_misses_total 1"), "{warm}");
    assert!(warm.contains(r#"pnpr_resolution_cache_hits_total{footprint="public"} 1"#));
    assert!(warm.contains(r#"pnpr_resolution_cache_hits_total{footprint="private"} 0"#));
    server.abort();
}

#[test]
fn tarball_route_metrics_count_gateway_urls_for_private_aliases() {
    let mut registry = registry_config();
    registry.routing.upstreams.insert(
        "corp".to_string(),
        upstream_with_access("https://npm.corp.example/", "$authenticated"),
    );
    let metrics = Arc::new(ServerMetrics::new());
    let router = tarball_router(&registry, user("alice")).with_metrics(Arc::clone(&metrics));

    router.route_lockfile(&config_for_registry("https://npm.corp.example/"), &lockfile("1.0.0"));

    let rendered = metrics.render_prometheus();
    assert!(rendered.contains(r#"pnpr_tarball_routes_total{route="gateway"} 1"#), "{rendered}");
    assert!(rendered.contains(r#"pnpr_tarball_routes_total{route="upstream"} 0"#));
    assert!(rendered.contains(r#"pnpr_tarball_routes_total{route="hosted"} 0"#));
}
