use super::{IndexAuth, parse_index, validate_index_credentials};
use pnpm_network::{AuthHeaders, UpstreamRouteHook};
use std::sync::Arc;

#[test]
fn credentialless_indexes_block_ancestor_credentials_for_pages_wheels_and_redirects() {
    let auth = AuthHeaders::default()
        .with_secure_transport()
        .with_route_hook(Arc::new(IndexAuth {
            indexes: [
                "https://parent:secret@example.test/simple/",
                "https://example.test/simple/public/",
                "https://child:secret@example.test/simple/child/",
            ]
            .into_iter()
            .map(|url| parse_index(url).unwrap())
            .collect(),
        }));
    assert!(auth.for_url("https://example.test/simple/alpha/").is_some());
    assert!(auth.for_url("https://example.test/simple/child/alpha/").is_some());
    for url in [
        "https://example.test/simple/public/alpha/",
        "https://example.test/simple/public/alpha.whl",
        "https://example.test/packages/alpha.whl",
        "https://example.test:8443/simple/alpha/",
        "https://other.test/simple/alpha/",
        "http://example.test/simple/alpha/",
    ] {
        assert_eq!(auth.for_url(url), None, "{url}");
    }
}

#[test]
fn index_auth_matches_directory_boundaries() {
    let auth = IndexAuth {
        indexes: vec![parse_index("https://user:secret@example.test/simple").unwrap()],
    };
    assert!(auth.authorize("https://example.test/simple/alpha/", None).is_some());
    assert_eq!(auth.authorize("https://example.test/simple-other/alpha/", None), None);
}

#[test]
fn duplicate_index_paths_reject_conflicting_logins_without_disclosing_credentials() {
    let first = "https://alice:alice-secret@example.test/simple/";
    for second in [
        "https://bob:bob-secret@example.test/simple/",
        "https://alice:rotated-secret@example.test/simple",
        "https://example.test/simple/",
        "https://bob:bob-secret@example.test/simple/?view=other",
    ] {
        let indexes = [first, second].map(|url| parse_index(url).unwrap());
        let error = validate_index_credentials(&indexes).unwrap_err();
        assert_eq!(
            error.code().unwrap().to_string(),
            "ERR_PNPM_CONFLICTING_PYTHON_INDEX_CREDENTIALS",
        );
        let message = error.to_string();
        assert!(message.contains("https://example.test/simple/"));
        for secret in ["alice", "bob", "secret", "view=other"] {
            assert!(!message.contains(secret), "{message}");
        }
    }
    let indexes = [first, "https://alice:alice-secret@example.test/simple"].map(|url| {
        parse_index(url).unwrap()
    });
    validate_index_credentials(&indexes).unwrap();
}
