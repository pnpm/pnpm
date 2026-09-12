use super::{
    BTreeMap, Ipv4Addr, PnprClient, ResolveProject, ResolveProjectsOptions, TcpListener,
    capture_one_request_with_response, deps, options,
};

#[tokio::test]
async fn multi_project_request_sends_every_workspace_project_without_a_synthetic_root() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind capture");
    let addr = listener.local_addr().expect("capture addr");
    let done = serde_json::json!({
        "type": "done",
        "lockfile": {
            "lockfileVersion": "9.0",
            "importers": {
                "packages/app": {},
                "packages/lib": {},
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
    opts.projects = vec![
        ResolveProject {
            dir: "packages/app".to_string(),
            name: Some("app".to_string()),
            version: Some("1.0.0".to_string()),
            dependencies: deps([("app-dependency", "1.0.0")]),
            dev_dependencies: BTreeMap::new(),
            optional_dependencies: BTreeMap::new(),
        },
        ResolveProject {
            dir: "packages/lib".to_string(),
            name: Some("lib".to_string()),
            version: Some("2.0.0".to_string()),
            dependencies: BTreeMap::new(),
            dev_dependencies: deps([("lib-tool", "2.0.0")]),
            optional_dependencies: BTreeMap::new(),
        },
    ];

    let outcome = client.resolve_projects(opts).await.expect("multi-project response should parse");
    let mut importer_ids = outcome.lockfile.importers.keys().cloned().collect::<Vec<_>>();
    importer_ids.sort();
    assert_eq!(importer_ids, ["packages/app".to_string(), "packages/lib".to_string()]);

    let request = capture.await.expect("capture task");
    let (_, body) = request.split_once("\r\n\r\n").expect("captured HTTP request has a body");
    let body: serde_json::Value = serde_json::from_str(body).expect("request body is JSON");
    assert_eq!(
        body["projects"],
        serde_json::json!([
            {
                "dir": "packages/app",
                "name": "app",
                "version": "1.0.0",
                "dependencies": { "app-dependency": "1.0.0" },
                "devDependencies": {},
                "optionalDependencies": {},
            },
            {
                "dir": "packages/lib",
                "name": "lib",
                "version": "2.0.0",
                "dependencies": {},
                "devDependencies": { "lib-tool": "2.0.0" },
                "optionalDependencies": {},
            },
        ]),
    );
}
