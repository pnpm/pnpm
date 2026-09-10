use super::{
    Duration, HashMap, Identity, Mutex, ResolveRequest, RouteContext, StatusCode,
    cached_resolution, config_for_registry, lockfile, lockfile_tarball_url, package_lockfile,
    private_alias_footprint, private_hosted_footprint, registry_config, reject_inline_url_auth,
    set_local_hosted_rules, store_resolution, tarball_router, upstream_with_access,
    upstream_with_token, user,
};

#[test]
fn private_cached_resolution_requires_current_alias_authorization() {
    let cache = Mutex::new(HashMap::new());
    let key = "base".to_string();
    let lockfile = lockfile("1.0.0");
    assert!(store_resolution(
        &cache,
        Duration::from_mins(1),
        key.clone(),
        private_alias_footprint("corp"),
        b"secret",
        &lockfile,
    ));

    let mut config = registry_config();
    config
        .upstreams
        .insert("corp".to_string(), upstream_with_access("https://npm.corp.example/", "alice"));
    let context = RouteContext::from_config(&config);
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("alice")).is_some(),
    );
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("bob")).is_none(),
    );

    // Rotate the upstream credential (new token → new credential digest). The
    // resolution cached under the old credential must no longer be reused, even
    // for a still-authorized caller.
    config.upstreams.insert(
        "corp".to_string(),
        upstream_with_token("https://npm.corp.example/", "alice", "Bearer rotated-secret"),
    );
    let rotated = RouteContext::from_config(&config);
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &rotated, &user("alice")).is_none(),
    );
}

#[test]
fn same_alias_authorized_users_share_private_resolution_cache() {
    let cache = Mutex::new(HashMap::new());
    let key = "base".to_string();
    let lockfile = lockfile("1.0.0");
    assert!(store_resolution(
        &cache,
        Duration::from_mins(1),
        key.clone(),
        private_alias_footprint("corp"),
        b"secret",
        &lockfile,
    ));

    let mut config = registry_config();
    config.upstreams.insert(
        "corp".to_string(),
        upstream_with_access("https://npm.corp.example/", "$authenticated"),
    );
    let context = RouteContext::from_config(&config);

    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("alice")).is_some(),
    );
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("bob")).is_some(),
    );
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &Identity::Anonymous,)
            .is_none(),
    );
}

#[test]
fn revoked_alias_access_stops_matching_private_resolution_hits() {
    let cache = Mutex::new(HashMap::new());
    let key = "base".to_string();
    let lockfile = lockfile("1.0.0");
    assert!(store_resolution(
        &cache,
        Duration::from_mins(1),
        key.clone(),
        private_alias_footprint("corp"),
        b"secret",
        &lockfile,
    ));

    let mut config = registry_config();
    config
        .upstreams
        .insert("corp".to_string(), upstream_with_access("https://npm.corp.example/", "alice"));
    let context = RouteContext::from_config(&config);
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("alice")).is_some(),
    );

    config
        .upstreams
        .insert("corp".to_string(), upstream_with_access("https://npm.corp.example/", "bob"));
    let context = RouteContext::from_config(&config);
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("alice")).is_none(),
    );
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("bob")).is_some(),
    );
}

#[test]
fn revoked_hosted_package_access_stops_matching_private_resolution_hits() {
    let cache = Mutex::new(HashMap::new());
    let key = "base".to_string();
    let lockfile = lockfile("1.0.0");
    assert!(store_resolution(
        &cache,
        Duration::from_mins(1),
        key.clone(),
        private_hosted_footprint("local", "@private/pkg"),
        b"secret",
        &lockfile,
    ));

    let mut config = registry_config();
    set_local_hosted_rules(&mut config, "@private/*", "alice");
    let context = RouteContext::from_config(&config);
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("alice")).is_some(),
    );
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("bob")).is_none(),
    );

    set_local_hosted_rules(&mut config, "@private/*", "bob");
    let context = RouteContext::from_config(&config);
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("alice")).is_none(),
    );
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("bob")).is_some(),
    );
}

#[test]
fn private_alias_lockfile_routing_uses_gateway_url() {
    let pnpm_config = config_for_registry("https://npm.corp.example/");
    let mut registry = registry_config();
    registry.upstreams.insert(
        "corp".to_string(),
        upstream_with_access("https://npm.corp.example/", "$authenticated"),
    );
    let router = tarball_router(&registry, user("alice"));

    let routed = router.route_lockfile(&pnpm_config, &lockfile("1.0.0"));
    let tarball = lockfile_tarball_url(&routed, "acme@1.0.0");

    assert!(tarball.starts_with("http://127.0.0.1:7677/~corp/acme/-/acme-1.0.0.tgz"));
    assert!(!tarball.contains("npm.corp.example"));

    let upstream = router.verification_lockfile(&routed);
    assert_eq!(
        lockfile_tarball_url(&upstream, "acme@1.0.0"),
        "https://npm.corp.example/acme/-/acme-1.0.0.tgz",
    );
}

#[test]
fn private_alias_lockfile_routing_encodes_scoped_packages_as_one_gateway_segment() {
    let pnpm_config = config_for_registry("https://npm.corp.example/");
    let mut registry = registry_config();
    registry.upstreams.insert(
        "corp".to_string(),
        upstream_with_access("https://npm.corp.example/", "$authenticated"),
    );
    let router = tarball_router(&registry, user("alice"));

    let routed = router.route_lockfile(&pnpm_config, &package_lockfile("@acme/foo", "1.0.0"));
    let tarball = lockfile_tarball_url(&routed, "@acme/foo@1.0.0");

    assert!(tarball.contains("/~corp/@acme/foo/-/foo-1.0.0.tgz"));
    assert!(!tarball.contains("npm.corp.example"));

    let upstream = router.verification_lockfile(&routed);
    assert_eq!(
        lockfile_tarball_url(&upstream, "@acme/foo@1.0.0"),
        "https://npm.corp.example/@acme/foo/-/foo-1.0.0.tgz",
    );
}

#[test]
fn reject_inline_url_auth_scans_catalogs() {
    let dirty_catalog = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "catalogs": {
            "default": { "foo": "https://user:pass@registry.example.test/foo.tgz" }
        }
    }))
    .expect("dirty catalog request parses");
    let clean_catalog = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "catalogs": { "default": { "foo": "https://registry.example.test/foo.tgz" } }
    }))
    .expect("clean catalog request parses");
    let ssh_catalog = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "catalogs": { "default": { "foo": "git+ssh://git@github.com/org/repo.git" } }
    }))
    .expect("ssh catalog request parses");

    let response =
        reject_inline_url_auth(&dirty_catalog).expect("inline catalog credentials are rejected");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    assert!(reject_inline_url_auth(&clean_catalog).is_none());
    // A bare ssh login username is not an inline credential (the off-allowlist
    // gate still decides whether the host may be fetched at all).
    assert!(reject_inline_url_auth(&ssh_catalog).is_none());
}
