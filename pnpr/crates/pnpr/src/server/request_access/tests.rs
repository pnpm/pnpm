use super::{Config, compute_upstream_cache_namespace};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use tempfile::TempDir;

fn config_from(yaml: &str) -> Config {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("pnpr.yaml");
    std::fs::write(&path, yaml).unwrap();
    let listen = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 4873));
    let mut config = Config::from_yaml(&path, listen, None).unwrap();
    config.resolution_cache_secret = b"fixed-secret".to_vec().into();
    config
}

#[test]
fn renaming_an_upstream_gives_it_a_separate_cache_namespace() {
    let top_level = config_from(
        "\
storage: ./s
registries:
  npmjs:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
  private-npm:
    type: upstream
    url: https://npm.example.com/
    access: $authenticated
  crates:
    type: upstream
    ecosystem: cargo
    url: https://index.crates.io/
    public: true
",
    );
    let renamed = config_from(
        "\
storage: ./s
registries:
  public-npm:
    type: upstream
    url: https://registry.npmjs.org/
    public: true
  private:
    type: upstream
    url: https://npm.example.com/
    access: $authenticated
  cratesio:
    type: upstream
    ecosystem: cargo
    url: https://index.crates.io/
    public: true
",
    );

    assert_ne!(
        compute_upstream_cache_namespace(&top_level, "npmjs"),
        compute_upstream_cache_namespace(&renamed, "public-npm"),
    );
    assert_ne!(
        compute_upstream_cache_namespace(&top_level, "private-npm"),
        compute_upstream_cache_namespace(&renamed, "private"),
    );
    assert_ne!(
        compute_upstream_cache_namespace(&top_level, "crates"),
        compute_upstream_cache_namespace(&renamed, "cratesio"),
    );
}

#[tokio::test]
async fn protocol_gates_use_the_resolved_identity() {
    use super::{require_artifact_caller, require_pipeline_caller, require_resolver_caller};
    use crate::server::AuthedCaller;
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        middleware,
        routing::get,
    };
    use pnpr_policy::Identity;
    use tower::ServiceExt;

    let app = Router::new()
        .route(
            "/resolve",
            get(|| async { "ok" }).route_layer(middleware::from_fn(require_resolver_caller)),
        )
        .route(
            "/artifacts",
            get(|| async { "ok" }).route_layer(middleware::from_fn(require_artifact_caller)),
        )
        .route(
            "/pipeline",
            get(|| async { "ok" }).route_layer(middleware::from_fn(require_pipeline_caller)),
        );
    for path in ["/resolve", "/artifacts", "/pipeline"] {
        for (caller, expected) in [
            (Some(Identity::user("account-one")), StatusCode::OK),
            (Some(Identity::Anonymous), StatusCode::UNAUTHORIZED),
            (None, StatusCode::INTERNAL_SERVER_ERROR),
        ] {
            let mut request = Request::builder()
                .uri(path)
                .body(Body::empty())
                .unwrap();
            if let Some(caller) = caller {
                request
                    .extensions_mut()
                    .insert(AuthedCaller(caller));
            }
            let response = app
                .clone()
                .oneshot(request)
                .await
                .unwrap();
            assert_eq!(response.status(), expected, "{path}");
        }
    }
}
