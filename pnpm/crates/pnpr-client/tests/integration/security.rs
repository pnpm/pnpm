use super::{
    BTreeMap, Ipv4Addr, PnprClient, ResolveProject, ResolveProjectsOptions, TcpListener,
    capture_one_request_with_response, options,
};

/// The resolved lockfile is merged into the caller's `pnpm-lock.yaml`, so
/// a server that answers with an importer nobody asked about would be
/// writing dependencies into an unrelated project. The response is
/// rejected rather than merged.
#[tokio::test]
async fn an_importer_outside_the_request_is_rejected() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind capture");
    let addr = listener.local_addr().expect("capture addr");
    let done = serde_json::json!({
        "type": "done",
        "lockfile": {
            "lockfileVersion": "9.0",
            "importers": {
                "packages/app": {},
                "packages/attacker": {},
            },
        },
        "stats": { "totalPackages": 0 },
    });
    let body = format!("{done}\n");
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    );
    let capture = tokio::spawn(capture_one_request_with_response(listener, response));

    let client = PnprClient::new(format!("http://{addr}/"));
    let mut opts: ResolveProjectsOptions =
        options("https://registry.example.test/", "Bearer token", BTreeMap::new()).into();
    opts.projects = vec![ResolveProject {
        dir: "packages/app".to_string(),
        name: Some("app".to_string()),
        version: Some("1.0.0".to_string()),
        dependencies: BTreeMap::new(),
        dev_dependencies: BTreeMap::new(),
        optional_dependencies: BTreeMap::new(),
    }];

    let error = match client.resolve_projects(opts).await {
        Ok(_) => panic!("an unrequested importer must not be accepted"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("packages/attacker"), "unexpected error: {error}");

    capture.await.expect("capture task");
}
