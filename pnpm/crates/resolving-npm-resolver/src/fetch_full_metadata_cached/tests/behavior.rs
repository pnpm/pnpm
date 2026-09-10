use super::{
    ABBREVIATED_META_DIR, ACCEPT_ABBREVIATED, AuthHeaders, FetchFullMetadataCachedOptions,
    PACKAGE_BODY, TempDir, ThrottledClient, fetch_full_metadata_cached, get_pkg_mirror_path,
    load_meta, no_retry_opts,
};

#[tokio::test]
async fn a_full_doc_served_for_an_abbreviated_request_is_normalized_before_caching() {
    let mut server = mockito::Server::new_async().await;
    // A full document: what a registry that ignores the abbreviated `Accept`
    // header (e.g. Azure DevOps Artifacts) serves. It carries per-version
    // fields the resolver never reads.
    let full_body = PACKAGE_BODY.replace(
        r#""dist": {"#,
        r#""readme": "drop me", "scripts": { "postinstall": "node install.js" }, "exports": { ".": "./index.js" }, "dependencies": { "bar": "^1.0.0" }, "dist": {"#,
    );
    let mock = server
        .mock("GET", "/acme")
        .match_header("accept", ACCEPT_ABBREVIATED)
        .with_status(200)
        // application/json (not the abbreviated content type) signals that
        // the registry ignored the abbreviated `Accept` header and served
        // the full document.
        .with_header("content-type", "application/json")
        .with_body(full_body)
        .expect(1)
        .create_async()
        .await;

    let cache = TempDir::new().expect("tempdir");
    let registry = format!("{}/", server.url());
    let http_client = ThrottledClient::default();
    let auth_headers = AuthHeaders::default();
    let opts = FetchFullMetadataCachedOptions {
        registry: &registry,
        http_client: &http_client,
        auth_headers: &auth_headers,
        cache_dir: Some(cache.path()),
        full_metadata: false,
        filter_metadata: false,
        offline: false,
        priority: pnpm_network::UNPRIORITIZED,
        retry_opts: no_retry_opts(),
    };

    let pkg = fetch_full_metadata_cached("acme", &opts).await.expect("200 → ok");
    assert_eq!(pkg.name, "acme");
    mock.assert_async().await;

    let mirror_path = get_pkg_mirror_path(cache.path(), ABBREVIATED_META_DIR, &registry, "acme")
        .expect("abbreviated path");
    let persisted = load_meta(&mirror_path).expect("mirror readable");
    let manifest = persisted.versions.get("1.0.0").expect("manifest");
    // Install-irrelevant fields dropped.
    assert!(!manifest.other.contains_key("readme"));
    assert!(!manifest.other.contains_key("scripts"));
    assert!(!manifest.other.contains_key("exports"));
    // Install-relevant fields kept, so resolution is unchanged.
    assert_eq!(
        manifest.dependencies.as_ref().and_then(|deps| deps.get("bar")).map(String::as_str),
        Some("^1.0.0"),
    );
}
