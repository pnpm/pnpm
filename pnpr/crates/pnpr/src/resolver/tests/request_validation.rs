use super::{
    ResolveRequest, RouteContext, StatusCode, registry_config, reject_off_allowlist_fetches,
};

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
