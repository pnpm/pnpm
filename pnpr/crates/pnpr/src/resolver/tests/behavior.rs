use super::{
    ALIAS_TOKEN, AccessList, Duration, Footprint, HashMap, Identity,
    MAX_RESOLUTION_CACHE_CANDIDATES_PER_KEY, Mutex, PrivateAccessDescriptor, ResolveRequest,
    RouteContext, cached_resolution, config, config_for_registry, deps, lockfile,
    private_alias_footprint, public_registry_config, registry_config, reject_inline_url_auth,
    reject_invalid_patch_hashes, reject_off_allowlist_fetches, resolution_cache_key,
    store_resolution, tarball_router, upstream_with_access, user,
};

/// The `registries` map is keyed by URL and carries declarations only. The
/// setting's older `<scope>: <url>` shape puts the URL in the *value*, where
/// the boundary checks — which read a key as a fetch target — would not see
/// it, so a request in that shape must not parse at all.
#[test]
fn a_scope_routed_registries_map_does_not_parse() {
    let declarations = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "registries": { "https://npm.corp.example/": { "scopes": ["@acme"] } }
    }))
    .expect("a declaration map parses");
    assert_eq!(declarations.registries.len(), 1);

    let scope_routed = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "registries": { "@acme": "http://169.254.169.254/" }
    }));
    assert!(scope_routed.is_err(), "a scope-routed map must not parse");
}

#[test]
fn patch_hashes_must_be_lowercase_sha256_digests() {
    let request = |hash: &str| {
        serde_json::from_value::<ResolveRequest>(serde_json::json!({
            "patchedDependencies": { "foo@1.0.0": hash }
        }))
        .expect("resolve request parses")
    };

    assert!(reject_invalid_patch_hashes(&request(&"a".repeat(64))).is_none());
    for invalid in ["abc".to_string(), "A".repeat(64), "g".repeat(64)] {
        assert!(reject_invalid_patch_hashes(&request(&invalid)).is_some(), "accepted {invalid}");
    }
}

#[test]
fn metadata_refreshes_bypass_the_resolution_cache() {
    let ordinary = ResolveRequest {
        dependencies: Some(deps(&[("foo", "1.0.0")])),
        ..ResolveRequest::default()
    };
    let refresh = ResolveRequest {
        dependencies: ordinary.dependencies.clone(),
        update_patches: true,
        ..ResolveRequest::default()
    };
    let repair = ResolveRequest {
        dependencies: ordinary.dependencies.clone(),
        fix_lockfile: true,
        ..ResolveRequest::default()
    };

    assert!(resolution_cache_key(&config(), &ordinary).is_some());
    assert!(resolution_cache_key(&config(), &refresh).is_none());
    assert!(resolution_cache_key(&config(), &repair).is_none());
}

#[test]
fn update_patches_defaults_to_false_for_older_clients() {
    let request = serde_json::from_value::<ResolveRequest>(serde_json::json!({}))
        .expect("legacy request parses");
    let refresh = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "updatePatches": true
    }))
    .expect("refresh request parses");

    assert!(!request.update_patches);
    assert!(refresh.update_patches);
}

#[test]
fn fix_lockfile_defaults_to_false_for_older_clients() {
    let request = serde_json::from_value::<ResolveRequest>(serde_json::json!({}))
        .expect("legacy request parses");
    let repair = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "fixLockfile": true
    }))
    .expect("repair request parses");

    assert!(!request.fix_lockfile);
    assert!(repair.fix_lockfile);
}

#[test]
fn public_cached_resolution_matches_every_caller() {
    let cache = Mutex::new(HashMap::new());
    let key = "base".to_string();
    let lockfile = lockfile("1.0.0");
    let context = RouteContext::from_config(&registry_config());

    assert!(store_resolution(
        &cache,
        Duration::from_mins(1),
        key.clone(),
        Footprint::default(),
        b"secret",
        &lockfile,
    ));

    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &Identity::Anonymous,)
            .is_some(),
    );
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("alice")).is_some(),
    );
}

#[test]
fn package_qualified_alias_descriptor_rechecks_upstream_rules_on_replay() {
    use pnpr_policy::{PackageRule, PackageRules};
    use pnpr_registry::{Ecosystem, PackagePattern};

    let cache = Mutex::new(HashMap::new());
    let key = "base".to_string();
    let lockfile = lockfile("1.0.0");
    // A resolution that touched an explicitly refined name records the
    // package-qualified descriptor (what `RouteHook` would produce).
    let mut footprint = Footprint::default();
    footprint.add(PrivateAccessDescriptor::Alias {
        alias: "corp".to_string(),
        credential_digest: pnpr_route::credential_digest(ALIAS_TOKEN),
        package: Some("@corp/secret".to_string()),
    });
    assert!(store_resolution(
        &cache,
        Duration::from_mins(1),
        key.clone(),
        footprint,
        b"secret",
        &lockfile,
    ));

    let mut config = registry_config();
    let mut upstream = upstream_with_access("https://npm.corp.example/", "$authenticated");
    upstream.rules = PackageRules::new(
        vec![PackageRule {
            pattern: PackagePattern::parse("@corp/secret", Ecosystem::Npm)
                .expect("test pattern parses"),
            access: Some(AccessList::from_tokens(["alice"])),
            publish: None,
            unpublish: None,
        }],
        Some(AccessList::from_tokens(["$authenticated"])),
    );
    config.upstreams.insert("corp".to_string(), upstream);
    let context = RouteContext::from_config(&config);

    // Alice satisfies the per-package refinement: the hit replays.
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("alice")).is_some(),
    );
    // Bob passes the registry-level alias gate but the refinement denies
    // him: replay must be exactly as strict as a fresh resolve, so no hit.
    assert!(
        cached_resolution(&cache, Duration::from_mins(1), &key, &context, &user("bob")).is_none(),
    );
}

#[test]
fn public_lockfile_routing_keeps_registry_resolutions_compact() {
    let registry = public_registry_config("https://registry.example.test/");
    let router = tarball_router(&registry, Identity::Anonymous);
    let routed = router.route_lockfile(&config(), &lockfile("1.0.0"));
    let value = serde_json::to_value(&routed).expect("lockfile serializes");

    assert_eq!(
        value["packages"]["acme@1.0.0"]["resolution"],
        serde_json::json!({
            "integrity": "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==",
        }),
    );
}

#[test]
fn unknown_lockfile_routing_leaves_resolution_unrewritten() {
    let pnpm_config = config_for_registry("https://unknown.example/");
    let registry = registry_config();
    let router = tarball_router(&registry, user("alice"));

    let input = lockfile("1.0.0");
    let routed = router.route_lockfile(&pnpm_config, &input);

    // An unknown route has no upstream and no managed credential, so pnpr mints
    // no gateway URL: the integrity-only registry resolution is left untouched
    // (the client fetches the upstream tarball directly, as it was resolved
    // anonymously), never rewritten into an explicit tarball URL.
    let value = serde_json::to_value(&routed).expect("lockfile serializes");
    let resolution = &value["packages"]["acme@1.0.0"]["resolution"];
    assert!(
        resolution.get("tarball").is_none(),
        "unknown route stays integrity-only: {resolution}",
    );
    assert_eq!(value, serde_json::to_value(&input).expect("lockfile serializes"));
}

#[test]
fn package_extensions_stay_inside_the_fetch_boundary() {
    let context = RouteContext::from_config(&registry_config());
    let off_allowlist = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "packageExtensions": {
            "foo@1.0.0": {
                "optionalDependencies": {
                    "bar": "https://169.254.169.254/bar.tgz"
                }
            }
        }
    }))
    .expect("package extension request parses");
    let inline_auth = serde_json::from_value::<ResolveRequest>(serde_json::json!({
        "packageExtensions": {
            "foo@1.0.0": {
                "peerDependencies": {
                    "bar": "https://user:pass@registry.example.test/bar.tgz"
                }
            }
        }
    }))
    .expect("package extension request parses");

    assert!(reject_off_allowlist_fetches(&off_allowlist, &context).is_some());
    assert!(reject_inline_url_auth(&inline_auth).is_some());
}

#[test]
fn candidate_lists_stay_bounded_and_keep_public_entries() {
    let cache = Mutex::new(HashMap::new());
    let key = "base".to_string();
    let lockfile = lockfile("1.0.0");
    assert!(store_resolution(
        &cache,
        Duration::from_mins(1),
        key.clone(),
        Footprint::default(),
        b"secret",
        &lockfile,
    ));
    for index in 0..(MAX_RESOLUTION_CACHE_CANDIDATES_PER_KEY + 2) {
        assert!(store_resolution(
            &cache,
            Duration::from_mins(1),
            key.clone(),
            private_alias_footprint(&format!("corp-{index}")),
            b"secret",
            &lockfile,
        ));
    }

    let cache = cache.lock().expect("resolution cache poisoned");
    let candidates = cache.get(&key).expect("base key remains cached");
    assert_eq!(candidates.len(), MAX_RESOLUTION_CACHE_CANDIDATES_PER_KEY);
    assert!(candidates.iter().any(|candidate| candidate.footprint.is_public()));
}
