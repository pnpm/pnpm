use super::{
    ResolveRequest, RouteContext, StatusCode, registry_config, reject_off_allowlist_fetches,
    reject_unsafe_publish_directories, reject_unusable_resolve,
};
use crate::resolver::{
    parse_request,
    protocol::{CargoResolveRequest, PypiResolveRequest},
};

async fn error_code(response: axum::response::Response) -> String {
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    body["code"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

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

#[tokio::test]
async fn reject_incompatible_lockfile_format_refuses_unsupported_versions() {
    let context = RouteContext::from_config(&registry_config());
    let incompatible = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "acceptedLockfileFormats": ["9.0", "pnpm-lock/13.0"]
    }))
    .expect("request parses");
    let response = reject_unusable_resolve(&incompatible, &context)
        .expect("incompatible lockfile format is rejected");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error_code(response).await, "ERR_PNPR_INCOMPATIBLE_LOCKFILE_FORMAT");

    let compatible = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "acceptedLockfileFormats": ["pnpm-lock/13.0", "pnpm-lock/9.0"]
    }))
    .expect("request parses");
    assert!(reject_unusable_resolve(&compatible, &context).is_none());
}

#[tokio::test]
async fn parse_request_refuses_a_field_the_server_does_not_know() {
    for body in [
        serde_json::json!({ "futureResolutionSetting": true }),
        serde_json::json!({ "projects": [{ "dir": ".", "futureProjectField": {} }] }),
        serde_json::json!({ "projects": [{ "dir": ".", "publishConfig": { "access": "public" } }] }),
    ] {
        let response = parse_request::<ResolveRequest>(body.to_string().as_bytes())
            .expect_err("unknown field is refused");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(error_code(response).await, "ERR_PNPR_UNSUPPORTED_REQUEST_FIELD");
    }

    let cargo = serde_json::json!({ "ecosystem": "cargo", "metadata": "{}", "features": [] });
    let response = parse_request::<CargoResolveRequest>(cargo.to_string().as_bytes())
        .expect_err("unknown cargo field is refused");
    assert_eq!(error_code(response).await, "ERR_PNPR_UNSUPPORTED_REQUEST_FIELD");

    let pypi = serde_json::json!({ "ecosystem": "pypi", "requirements": [], "extras": [] });
    let response = parse_request::<PypiResolveRequest>(pypi.to_string().as_bytes())
        .expect_err("unknown pypi field is refused");
    assert_eq!(error_code(response).await, "ERR_PNPR_UNSUPPORTED_REQUEST_FIELD");
}

#[tokio::test]
async fn parse_request_keeps_malformed_bodies_apart_from_unknown_fields() {
    let response = parse_request::<ResolveRequest>(br#"{ "frozenLockfile": "yes" }"#)
        .expect_err("a mistyped field is refused");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error_code(response).await, "ERR_PNPR_BAD_REQUEST");
}

#[test]
fn parse_request_accepts_the_fields_existing_clients_send() {
    let body = serde_json::json!({
        "ecosystem": "npm",
        "nodeVersion": "22.0.0",
        "os": "linux",
        "arch": "x64",
        "acceptedLockfileFormats": ["pnpm-lock/9.0"],
    });
    assert!(parse_request::<ResolveRequest>(body.to_string().as_bytes()).is_ok());
}
