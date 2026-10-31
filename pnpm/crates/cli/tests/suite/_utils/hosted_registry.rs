//! A hosted in-process pnpr for the tests that publish. The shared
//! `pnpm_testing_utils` registry runs in proxy mode and rejects publishes, so
//! a test that needs to publish serves a static-mode instance of its own.

use pnpr_fixtures::MockConfigPresets;
use serde_json::{Value, json};

/// A hosted pnpr on an ephemeral localhost port, backed by a fresh storage
/// tempdir (returned so it outlives the test), and its npm registry URL.
pub fn spawn_hosted_registry() -> (String, tempfile::TempDir) {
    let storage = tempfile::tempdir().expect("registry storage");
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("bind the hosted registry to an unused localhost port");
    listener.set_nonblocking(true).expect("set the registry listener to nonblocking");
    let listen = listener.local_addr().expect("read the registry listener address");
    let url = format!("http://{listen}/");
    let mut config = pnpr::Config::static_serve(listen, storage.path().to_path_buf());
    config.http.public_url = url.trim_end_matches('/').to_string();
    config.identity.auth.htpasswd.max_users = pnpr::MaxUsers::Unlimited;
    std::thread::Builder::new()
        .name("hosted-registry".to_string())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("create the registry runtime");
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener)
                    .expect("create the registry tokio listener");
                pnpr::serve_listener(config, listener).await.expect("serve the hosted registry");
            });
        })
        .expect("spawn the registry thread");
    (format!("{url}npm/"), storage)
}

/// Run one async request against the hosted registry from a synchronous test.
pub fn block_on<Output>(future: impl std::future::Future<Output = Output>) -> Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("create the request runtime")
        .block_on(future)
}

/// An HTTP client whose requests time out instead of hanging the test run
/// if the hosted registry stops responding.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_mins(1))
        .build()
        .expect("build the test HTTP client")
}

/// Register a user against the hosted registry and return their bearer token.
pub fn add_user(registry: &str) -> String {
    let url = format!("{registry}-/user/org.couchdb.user:alice");
    let body = json!({
        "_id": "org.couchdb.user:alice",
        "name": "alice",
        "password": "secret",
        "email": "alice@example.com",
        "type": "user",
        "roles": [],
    });
    block_on(async {
        let response = http_client()
            .put(&url)
            .json(&body)
            .send()
            .await
            .expect("send the adduser request");
        assert_eq!(response.status().as_u16(), 201, "adduser must succeed");
        let payload: Value = response.json().await.expect("parse the adduser response");
        payload["token"]
            .as_str()
            .expect("token in the adduser response")
            .to_owned()
    })
}

/// The `.npmrc` line carrying `token` for `registry`, written to `path`.
pub fn write_auth_file(path: &std::path::Path, registry: &str, token: &str) {
    let host = registry.strip_prefix("http://").unwrap_or(registry);
    std::fs::write(path, format!("//{host}:_authToken={token}\n")).expect("write auth .npmrc");
}
