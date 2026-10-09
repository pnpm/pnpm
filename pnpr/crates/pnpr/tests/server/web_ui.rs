use super::{
    Body, Config, GzDecoder, Ipv4Addr, Request, ServiceExt, SocketAddr, SocketAddrV4, StatusCode,
    TempDir, body_bytes, header, router,
};

const INDEX: &str = "<!doctype html><head><title>pnpr</title></head>";

fn page_with_base(root: &str) -> Vec<u8> {
    INDEX
        .replace("<head>", &format!(r#"<head><base href="{root}">"#))
        .into_bytes()
}

fn config_with_ui(storage: &TempDir, ui: pnpr::UiConfig) -> Config {
    let mut config = Config::static_serve(
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
        storage.path().to_path_buf(),
    );
    config.http.ui = ui;
    config
}

fn built_ui() -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("index.html"), INDEX).unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("assets/index-abc.js"), "console.log(1)").unwrap();
    dir
}

fn ui_at(dir: &TempDir) -> pnpr::UiConfig {
    pnpr::UiConfig { enabled: true, dir: Some(dir.path().to_path_buf()) }
}

async fn get(app: &axum::Router, uri: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::get(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn serves_the_ui_and_its_routes_from_index_html() {
    let storage = TempDir::new().unwrap();
    let ui = built_ui();
    let app = router(config_with_ui(&storage, ui_at(&ui)));

    let redirect = get(&app, "/-/ui").await;
    assert!(redirect.status().is_redirection());
    assert_eq!(redirect.headers()[header::LOCATION], "ui/");

    for (uri, root) in [
        ("/-/ui/", "./"),
        ("/-/ui/npm?q=react", "./"),
        ("/-/ui/npm/lodash.merge", "../"),
        ("/-/ui/npm/@scope/name/", "../../../"),
    ] {
        let response = get(&app, uri).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "text/html; charset=utf-8");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
        assert_eq!(response.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        let policy = response.headers()[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
        assert!(policy.contains("script-src 'self';"), "{policy}");
        assert!(policy.contains("connect-src 'self';"), "{policy}");
        assert!(policy.contains("base-uri 'self';"), "{policy}");
        assert_eq!(body_bytes(response.into_body()).await, page_with_base(root));
    }
}

#[tokio::test]
async fn serves_hashed_assets_as_immutable_and_404s_a_missing_one() {
    let storage = TempDir::new().unwrap();
    let ui = built_ui();
    let app = router(config_with_ui(&storage, ui_at(&ui)));

    let asset = get(&app, "/-/ui/assets/index-abc.js").await;
    assert_eq!(asset.status(), StatusCode::OK);
    assert_eq!(asset.headers()[header::CONTENT_TYPE], "text/javascript; charset=utf-8");
    assert_eq!(asset.headers()[header::CACHE_CONTROL], "public, max-age=31536000, immutable");
    assert_eq!(body_bytes(asset.into_body()).await, b"console.log(1)".as_slice());

    let missing = get(&app, "/-/ui/assets/index-old.js").await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(missing.headers()[header::CACHE_CONTROL], "no-cache");
}

#[tokio::test]
async fn never_serves_a_file_outside_the_ui_directory() {
    let storage = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    std::fs::write(root.path().join("secret.txt"), "secret").unwrap();
    let ui_dir = root.path().join("ui");
    std::fs::create_dir(&ui_dir).unwrap();
    std::fs::write(ui_dir.join("index.html"), INDEX).unwrap();
    let ui = pnpr::UiConfig { enabled: true, dir: Some(ui_dir) };
    let app = router(config_with_ui(&storage, ui));

    for uri in [
        "/-/ui/../secret.txt",
        "/-/ui/..%2fsecret.txt",
        "/-/ui/..%5csecret.txt",
        r"/-/ui/assets\..\..\secret.txt",
        "/-/ui/assets/../../secret.txt",
    ] {
        let response = get(&app, uri).await;
        let body = body_bytes(response.into_body()).await;
        assert!(
            !body
                .windows(6)
                .any(|window| window == b"secret"),
            "{uri}",
        );
    }
}

#[tokio::test]
async fn a_disabled_ui_is_not_served() {
    let storage = TempDir::new().unwrap();
    let ui = built_ui();
    let disabled = pnpr::UiConfig { enabled: false, dir: Some(ui.path().to_path_buf()) };
    let app = router(config_with_ui(&storage, disabled));

    let response = get(&app, "/-/ui/").await;
    assert!(
        response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .is_none(),
    );
    assert_ne!(body_bytes(response.into_body()).await, page_with_base("./"));
}

#[tokio::test]
async fn a_ui_dir_without_index_html_is_a_config_error() {
    let storage = TempDir::new().unwrap();
    let empty = TempDir::new().unwrap();
    let error = pnpr::try_router(config_with_ui(&storage, ui_at(&empty))).unwrap_err();
    assert!(error.to_string().contains("has no index.html"), "{error}");
}

#[cfg(unix)]
#[tokio::test]
async fn never_follows_a_symlink_out_of_the_ui_directory() {
    let storage = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    std::fs::write(root.path().join("secret.txt"), "secret").unwrap();
    let ui_dir = root.path().join("ui");
    std::fs::create_dir_all(ui_dir.join("assets")).unwrap();
    std::fs::write(ui_dir.join("index.html"), INDEX).unwrap();
    std::os::unix::fs::symlink(root.path().join("secret.txt"), ui_dir.join("leak.txt")).unwrap();
    std::os::unix::fs::symlink(root.path().join("secret.txt"), ui_dir.join("assets/leak.js"))
        .unwrap();
    let ui = pnpr::UiConfig { enabled: true, dir: Some(ui_dir) };
    let app = router(config_with_ui(&storage, ui));

    let page = get(&app, "/-/ui/leak.txt").await;
    assert_eq!(body_bytes(page.into_body()).await, page_with_base("./"));
    let asset = get(&app, "/-/ui/assets/leak.js").await;
    assert_eq!(asset.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn serves_a_large_asset_compressed() {
    let storage = TempDir::new().unwrap();
    let ui = built_ui();
    let script = "console.log('pnpr');\n".repeat(4096);
    std::fs::write(ui.path().join("assets/index-big.js"), &script).unwrap();
    let app = router(config_with_ui(&storage, ui_at(&ui)));

    let response = app
        .oneshot(
            Request::get("/-/ui/assets/index-big.js")
                .header(header::ACCEPT_ENCODING, "gzip")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_ENCODING], "gzip");
    let gzipped = body_bytes(response.into_body()).await;
    let mut decoded = String::new();
    std::io::Read::read_to_string(&mut GzDecoder::new(&gzipped[..]), &mut decoded).unwrap();
    assert_eq!(decoded, script);
}
