use super::{
    ResolveRequest, RouteContext, StatusCode, registry_config, reject_off_allowlist_fetches,
    reject_unaccepted_lockfile_format, reject_unsafe_publish_directories, reject_unusable_resolve,
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

#[test]
fn reject_incompatible_lockfile_format_refuses_unsupported_versions() {
    let context = RouteContext::from_config(&registry_config());
    let incompatible = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "acceptedLockfileFormats": ["6.0", "7.0"]
    }))
    .expect("request parses");
    let response = reject_unusable_resolve(&incompatible, &context)
        .expect("incompatible lockfile format is rejected");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let compatible = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "acceptedLockfileFormats": ["6.0", "9.0"]
    }))
    .expect("request parses");
    assert!(reject_unusable_resolve(&compatible, &context).is_none());

    let compatible = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "acceptedLockfileFormats": ["13.0"]
    }))
    .expect("request parses");
    assert!(reject_unusable_resolve(&compatible, &context).is_none());
}

#[test]
fn rejects_a_resolved_format_the_client_did_not_accept() {
    let lockfile: pnpm_lockfile::Lockfile =
        serde_saphyr::from_str("lockfileVersion: '13.0'\nimporters: {}\n")
            .expect("format 13 lockfile parses");
    let legacy = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "acceptedLockfileFormats": ["9.0"]
    }))
    .expect("request parses");
    let response = reject_unaccepted_lockfile_format(&legacy, &lockfile)
        .expect("unaccepted format is rejected");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let unmarked_variants: pnpm_lockfile::Lockfile = serde_saphyr::from_str(
        "lockfileVersion: '9.0'\nimporters: {}\npackages:\n  foo@1.0.0:\n    resolution:\n      type: variations\n      variants: []\n",
    )
    .expect("unmarked variations lockfile parses");
    assert!(reject_unaccepted_lockfile_format(&legacy, &unmarked_variants).is_some());

    let capable = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "acceptedLockfileFormats": ["9.0", "13.0"]
    }))
    .expect("request parses");
    assert!(reject_unaccepted_lockfile_format(&capable, &lockfile).is_none());

    let incompatible = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "acceptedLockfileFormats": ["13.0"]
    }))
    .expect("request parses");
    let standard: pnpm_lockfile::Lockfile =
        serde_saphyr::from_str("lockfileVersion: '9.0'\nimporters: {}\n")
            .expect("format 9 lockfile parses");
    assert!(reject_unaccepted_lockfile_format(&incompatible, &standard).is_some());

    let compatible_12 = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "acceptedLockfileFormats": ["12.0"]
    }))
    .expect("request parses");
    let lockfile_12: pnpm_lockfile::Lockfile =
        serde_saphyr::from_str("lockfileVersion: '12.0'\nimporters: {}\n")
            .expect("format 12 lockfile parses");
    assert!(reject_unaccepted_lockfile_format(&compatible_12, &lockfile_12).is_none());

    let legacy = serde_json::from_value::<ResolveRequest>(serde_json::json!({}))
        .expect("legacy request parses");
    assert!(reject_unaccepted_lockfile_format(&legacy, &lockfile_12).is_none());
    assert!(
        reject_unaccepted_lockfile_format(&legacy, &lockfile).is_some(),
        "a client that names no formats has not accepted the variants format",
    );

    let shorthand = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "acceptedLockfileFormats": ["13"]
    }))
    .expect("request parses");
    assert!(reject_unaccepted_lockfile_format(&shorthand, &lockfile).is_none());
}

#[test]
fn a_runtimes_archives_need_no_variants_format() {
    let runtime: pnpm_lockfile::Lockfile = serde_saphyr::from_str(concat!(
        "lockfileVersion: '9.0'\n",
        "importers: {}\n",
        "packages:\n",
        "  node@runtime:22.0.0:\n",
        "    resolution:\n",
        "      type: variations\n",
        "      variants:\n",
        "        - resolution:\n",
        "            type: binary\n",
        "            url: https://nodejs.org/dist/v22.0.0/node-v22.0.0-linux-x64.tar.gz\n",
        "            integrity: sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg==\n",
        "            bin: bin/node\n",
        "            archive: tarball\n",
        "          targets:\n",
        "            - os: linux\n",
        "              cpu: x64\n",
    ))
    .expect("runtime lockfile parses");
    for formats in
        [serde_json::json!({ "acceptedLockfileFormats": ["9.0"] }), serde_json::json!({})]
    {
        let request = serde_json::from_value::<ResolveRequest>(formats).expect("request parses");
        assert!(reject_unaccepted_lockfile_format(&request, &runtime).is_none());
    }
}
