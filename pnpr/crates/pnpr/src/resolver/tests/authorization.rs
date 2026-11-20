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
    config.routing.upstreams.insert(
        "corp".to_string(),
        upstream_with_access("https://npm.corp.example/", "alice"),
    );
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
    config.routing.upstreams.insert(
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
    config.routing.upstreams.insert(
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
    config.routing.upstreams.insert(
        "corp".to_string(),
        upstream_with_access("https://npm.corp.example/", "alice"),
    );
    let context = RouteContext::from_config(&config);
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("alice")).is_some(),
    );

    config.routing.upstreams.insert(
        "corp".to_string(),
        upstream_with_access("https://npm.corp.example/", "bob"),
    );
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
    registry.routing.upstreams.insert(
        "corp".to_string(),
        upstream_with_access("https://npm.corp.example/", "$authenticated"),
    );
    let router = tarball_router(&registry, user("alice"));

    let routed = router.route_lockfile(&pnpm_config, &lockfile("1.0.0"));
    let tarball = lockfile_tarball_url(&routed, "acme@1.0.0");

    assert!(tarball.starts_with("http://127.0.0.1:7677/npm/~corp/acme/-/acme-1.0.0.tgz"));
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
    registry.routing.upstreams.insert(
        "corp".to_string(),
        upstream_with_access("https://npm.corp.example/", "$authenticated"),
    );
    let router = tarball_router(&registry, user("alice"));

    let routed = router.route_lockfile(&pnpm_config, &package_lockfile("@acme/foo", "1.0.0"));
    let tarball = lockfile_tarball_url(&routed, "@acme/foo@1.0.0");

    assert!(tarball.contains("/npm/~corp/@acme/foo/-/foo-1.0.0.tgz"));
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

#[tokio::test]
async fn warm_resolve_retains_private_footprint_from_verdict_cache_hit() {
    use crate::resolver::{
        Resolver,
        dispatch::handle_resolve,
        resolution_cache_key,
        verdict_cache::{VerdictLookup, VerdictRecord},
        verify_lockfile::handle_verify_lockfile,
    };
    use axum::body::{Bytes, to_bytes};
    use pnpm_lockfile_verification::hash_lockfile;
    use pnpr_config::{IpNetwork, PublicRoute};
    use serde_json::json;
    use tokio::{
        io::{AsyncReadExt as _, AsyncWriteExt as _},
        net::TcpListener,
    };

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server_origin = origin.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0_u8; 512];
            let read = socket.read(&mut buf).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..read]);
            let name = if head.contains("public-dep") { "public-dep" } else { "@private/pkg" };
            let packument = json!({
                "name": name,
                "dist-tags": { "latest": "1.0.0" },
                "versions": {
                    "1.0.0": {
                        "name": name,
                        "version": "1.0.0",
                        "dist": {
                            "tarball": format!("{server_origin}/pkg.tgz"),
                            "integrity": ssri::Integrity::from(b"pkg-bytes").to_string(),
                        },
                    },
                },
            })
            .to_string();
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{packument}",
                packument.len(),
            );
            let _ = socket.write_all(reply.as_bytes()).await;
        }
    });

    let dir = tempfile::tempdir().unwrap();
    let mut config = registry_config();
    config.storage.cache_dir = dir.path().to_path_buf();
    config.http.public_url = "http://example.test".into();
    config.routing.route_policy.public.push(PublicRoute {
        registry: Some(format!("{origin}/")),
        package: None,
    });
    config.routing.route_policy.allowed_private_networks =
        vec![IpNetwork::parse("127.0.0.0/8").unwrap()];
    set_local_hosted_rules(&mut config, "@private/*", "alice");
    let resolver = Resolver::build(&config, None);

    let input_lockfile = package_lockfile("@private/pkg", "1.0.0");
    let payload = json!({
        "registry": format!("{origin}/"),
        "dependencies": { "@private/pkg": "^1.0.0", "public-dep": "^1.0.0" },
        "lockfile": input_lockfile,
        "frozenLockfile": false,
        "trustPolicy": "off",
        "minimumReleaseAge": 0,
    });
    let request: ResolveRequest = serde_json::from_value(payload.clone()).unwrap();
    let verified =
        handle_verify_lockfile(&resolver, user("alice"), Bytes::from(payload.to_string())).await;
    assert_eq!(verified.status(), StatusCode::OK);

    let hash = hash_lockfile(
        &tarball_router(&config, user("alice"))
            .verification_lockfile(request.lockfile.as_ref().unwrap()),
    );
    let epoch = resolver.local_registry.verdict_epoch().expect("stable epoch");
    let verdicts = resolver.cache.verdicts.as_ref().expect("verdict cache");
    let captured_policy = std::cell::RefCell::new(None);
    assert!(
        verdicts
            .verified_footprint(
                &VerdictLookup {
                    hash: &hash,
                    epoch: &epoch,
                    secret: &resolver.cache.secret,
                    route_context: &resolver.route_context,
                    identity: &user("alice"),
                },
                |policy| {
                    *captured_policy.borrow_mut() = Some(policy.clone());
                    true
                },
            )
            .is_some(),
    );
    let expected_footprint = private_hosted_footprint("local", "@private/pkg");
    verdicts.clear();
    verdicts.record(VerdictRecord {
        hash: &hash,
        footprint: &expected_footprint,
        secret: &resolver.cache.secret,
        epoch: &epoch,
        policy: captured_policy.into_inner().expect("policy recorded"),
    });

    let response = handle_resolve(&resolver, user("alice"), Bytes::from(payload.to_string())).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body =
        String::from_utf8(to_bytes(response.into_body(), usize::MAX).await.unwrap().to_vec())
            .unwrap();
    assert!(body.contains(r#""type":"done""#), "{body}");

    let raw_key = resolution_cache_key(&request).expect("cacheable request");
    let key = resolver.scoped_cache_key(resolver.cache_generation(), &raw_key);
    {
        let entries = resolver.cache.entries.lock().unwrap();
        let candidates = entries.get(&key).expect("warm resolution cached");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].footprint, expected_footprint);
    }
    assert!(
        cached_resolution(
            &resolver.cache.entries,
            resolver.cache.ttl,
            &key,
            &resolver.route_context,
            &user("alice"),
        )
        .is_some(),
    );
    assert!(
        cached_resolution(
            &resolver.cache.entries,
            resolver.cache.ttl,
            &key,
            &resolver.route_context,
            &user("bob"),
        )
        .is_none(),
    );
    server.abort();
}
