use super::{
    ResolveRequest, RouteContext, StatusCode, registry_config, reject_off_allowlist_fetches,
    reject_unsafe_publish_directories, reject_unusable_resolve,
};

#[test]
fn reject_unsafe_publish_directories_only_refuses_escapes() {
    let request = |directory: &str| {
        serde_json::from_value::<ResolveRequest>(serde_json::json!({
            "projects": [
                {
                    "dir": "packages/lib",
                    "name": "lib",
                    "version": "1.2.3",
                    "publishConfig": { "directory": directory }
                }
            ]
        }))
        .expect("publish directory request parses")
    };

    // The resolver joins this value onto the project dir, so nothing that could
    // leave the project may reach that join.
    for escapes in ["../outside", "/outside", "dist/../../outside", r"..\outside", "C:/outside"] {
        let response = reject_unsafe_publish_directories(&request(escapes))
            .expect("a directory that escapes its project is rejected");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    // A local install resolves these, so the pnpr path must accept them too.
    for contained in ["dist", "./dist", "dist/nested"] {
        assert!(
            reject_unsafe_publish_directories(&request(contained)).is_none(),
            "{contained:?} stays inside its project",
        );
    }

    let without_publish_config = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "projects": [{ "dir": ".", "name": "app", "version": "1.0.0" }]
    }))
    .expect("request without a publish directory parses");
    assert!(reject_unsafe_publish_directories(&without_publish_config).is_none());
}

#[test]
fn reject_unusable_resolve_refuses_an_escaping_publish_directory() {
    let context = RouteContext::from_config(&registry_config());
    let request = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "projects": [
            {
                "dir": "packages/lib",
                "name": "lib",
                "version": "1.2.3",
                "publishConfig": { "directory": "../outside" }
            }
        ]
    }))
    .expect("publish directory request parses");

    // `resolve_npm_request` runs this chain before every resolve path, including
    // the frozen fast path that returns the caller's input lockfile without
    // walking the tree, so an escaping value cannot skip the check.
    let response = reject_unusable_resolve(&request, &context)
        .expect("the request chain refuses an escaping publish directory");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn reject_off_allowlist_fetches_scans_catalogs() {
    let context = RouteContext::from_config(&registry_config());
    let default_catalog = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "catalogs": { "default": { "foo": "https://169.254.169.254/foo.tgz" } }
    }))
    .expect("default catalog request parses");
    let named_catalog = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "catalogs": { "internal": { "foo": "git+ssh://169.254.169.254/repo.git" } }
    }))
    .expect("named catalog request parses");
    let clean_catalog = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "catalogs": { "default": { "foo": "^1.0.0" } }
    }))
    .expect("clean catalog request parses");

    let default_response = reject_off_allowlist_fetches(&default_catalog, &context)
        .expect("default catalog URL is rejected");
    assert_eq!(default_response.status(), StatusCode::FORBIDDEN);

    let named_response = reject_off_allowlist_fetches(&named_catalog, &context)
        .expect("named catalog URL is rejected");
    assert_eq!(named_response.status(), StatusCode::FORBIDDEN);

    assert!(reject_off_allowlist_fetches(&clean_catalog, &context).is_none());
}
