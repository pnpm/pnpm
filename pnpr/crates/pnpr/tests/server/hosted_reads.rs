use super::{
    AccessList, AuthState, Body, Ecosystem, PackagePattern, Registries, Registry, Request,
    ServiceExt, StatusCode, TempDir, access_rule, body_bytes, body_json, config_for, header,
    hosted_publish_request, hosted_with_access, integrity_addressed_tarball_path, json,
    router_with_auth, seed_hosted, sha512_integrity,
};

#[tokio::test]
async fn hosted_registry_serves_only_what_it_hosts() {
    let tmp = TempDir::new().unwrap();
    // The org "acme" stores under its own namespace (`<storage>/acme/`).
    seed_hosted(&tmp.path().join("acme"), "@acme/widget");

    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.hosted.insert("acme".to_string(), hosted_with_access("acme", "$all"));
    config.registries = Registries::new(
        vec![("acme".to_string(), Registry::Hosted { patterns: vec![] })].into_iter().collect(),
        None,
    );
    let app = router_with_auth(config, AuthState::in_memory());

    // A hosted package is served from the org registry.
    let hit = app
        .clone()
        .oneshot(Request::get("/~acme/@acme/widget").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(hit.status(), StatusCode::OK);

    // A name the org does not host is a definitive not-found — a hosted org has
    // no upstream fall-through.
    let miss = app
        .oneshot(Request::get("/~acme/@acme/absent").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(miss.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn private_hosted_hides_existence_from_unauthorized_caller() {
    let tmp = TempDir::new().unwrap();
    // The org "acme" stores under its own namespace (`<storage>/acme/`).
    seed_hosted(&tmp.path().join("acme"), "@acme/widget");

    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.hosted.insert("acme".to_string(), hosted_with_access("acme", "alice"));
    config.registries = Registries::new(
        vec![("acme".to_string(), Registry::Hosted { patterns: vec![] })].into_iter().collect(),
        None,
    );
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config, auth);

    // An unauthorized (anonymous) caller gets 404, not 403 — the org's package
    // existence is not revealed.
    let anon = app
        .clone()
        .oneshot(Request::get("/~acme/@acme/widget").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(anon.status(), StatusCode::NOT_FOUND);

    // The authorized caller reads it.
    let authed = app
        .oneshot(
            Request::get("/~acme/@acme/widget")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(authed.status(), StatusCode::OK);
}

/// A private org's 404 mask must win over a *denying per-package ACL* on every
/// registry-served read path: otherwise the ACL's 401/403 would fire first and
/// reveal that the package exists.
#[tokio::test]
async fn private_hosted_org_masks_before_the_package_acl() {
    let tmp = TempDir::new().unwrap();
    seed_hosted(&tmp.path().join("acme"), "@acme/widget");

    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.hosted.insert("acme".to_string(), hosted_with_access("acme", "alice"));
    config.registries = Registries::new(
        vec![("acme".to_string(), Registry::Hosted { patterns: vec![] })].into_iter().collect(),
        None,
    );
    // A per-package rule that also denies the anonymous caller. In the
    // merged model there is no separate ACL layer whose ordering could
    // leak: a caller the registry-level default denies is masked as
    // not-found for every name, explicitly ruled or not.
    config
        .hosted
        .get_mut("acme")
        .expect("hosted acme")
        .rules
        .push_rule(access_rule("@acme/widget", "$authenticated"));
    let app = router_with_auth(config, AuthState::in_memory());

    for path in [
        "/~acme/@acme/widget",                    // packument
        "/~acme/@acme/widget/1.0.0",              // version manifest
        "/~acme/@acme/widget/-/widget-1.0.0.tgz", // tarball
    ] {
        let resp =
            app.clone().oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "existence leaked on {path}");
    }
}

/// The same 404-before-ACL masking must hold on the path-less `dist-tags` reader
/// (`load_packument_for_read`), which resolves through the default-target registry.
#[tokio::test]
async fn private_hosted_org_masks_dist_tags_before_the_package_acl() {
    let tmp = TempDir::new().unwrap();
    seed_hosted(&tmp.path().join("acme"), "@acme/widget");

    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.hosted.insert("acme".to_string(), hosted_with_access("acme", "alice"));
    // The path-less base aliases the "acme" hosted registry, so `/-/package/...`
    // resolves to it.
    config.registries = Registries::new(
        vec![("acme".to_string(), Registry::Hosted { patterns: vec![] })].into_iter().collect(),
        Some("acme".to_string()),
    );
    config
        .hosted
        .get_mut("acme")
        .expect("hosted acme")
        .rules
        .push_rule(access_rule("@acme/widget", "$authenticated"));
    let app = router_with_auth(config, AuthState::in_memory());

    let resp = app
        .oneshot(Request::get("/-/package/@acme%2Fwidget/dist-tags").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn publish_to_hosted_round_trips_in_its_own_namespace() {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.hosted.insert("acme".to_string(), hosted_with_access("acme", "$authenticated"));
    // npmjs already exists (from config_for); add a hosted org claiming
    // `@acme/*` + a router over it and the pattern-less npmjs catch-all,
    // aliased path-less.
    let graph = vec![
        ("npmjs".to_string(), Registry::Upstream { patterns: vec![] }),
        (
            "acme".to_string(),
            Registry::Hosted {
                patterns: vec![PackagePattern::parse("@acme/*", Ecosystem::Npm).unwrap()],
            },
        ),
        (
            "main".to_string(),
            Registry::Router { sources: vec!["acme".to_string(), "npmjs".to_string()] },
        ),
    ];
    config.registries = Registries::new(graph.into_iter().collect(), Some("main".to_string()));
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config, auth);

    let tarball = b"acme-widget-1.0.0-bytes";
    let body = json!({
        "name": "@acme/widget",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": {
            "name": "@acme/widget",
            "version": "1.0.0",
            "_npmUser": { "name": "mallory" },
            "dist": {
                "tarball": "http://example.test/@acme/widget/-/widget-1.0.0.tgz",
                "integrity": sha512_integrity(tarball),
            },
        } },
        "_attachments": { "@acme/widget-1.0.0.tgz": {
            "content_type": "application/octet-stream",
            "data": BASE64.encode(tarball),
            "length": tarball.len(),
        } },
    });

    // Publish through the org's own `/~acme/` endpoint.
    let publish = app
        .clone()
        .oneshot(
            Request::put("/~acme/@acme/widget")
                .header("content-type", "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);

    // It physically lands in the org's storage namespace, not the flat root.
    assert!(
        tmp.path().join("acme/@acme/widget/package.json").exists(),
        "published packument should be in the org namespace",
    );
    assert!(
        !tmp.path().join("@acme/widget/package.json").exists(),
        "nothing should be written to the flat hosted root",
    );

    // It reads back through the org endpoint (authenticated)...
    let read = app
        .clone()
        .oneshot(
            Request::get("/~acme/@acme/widget")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(read.status(), StatusCode::OK);
    let doc = body_json(read.into_body()).await;
    assert!(doc["versions"]["1.0.0"].is_object());
    assert_eq!(
        doc["versions"]["1.0.0"]["_npmUser"]["name"],
        json!("alice"),
        "the authenticated publisher must replace client-supplied attribution",
    );

    let search = app
        .clone()
        .oneshot(
            Request::get("/~acme/-/v1/search?text=maintainer%3Aalice")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(search.status(), StatusCode::OK);
    let search = body_json(search.into_body()).await;
    assert_eq!(search["total"], json!(1));
    assert_eq!(search["objects"][0]["package"]["name"], json!("@acme/widget"));

    // ...and its tarball serves from the org namespace.
    let tar = app
        .clone()
        .oneshot(
            Request::get("/~acme/@acme/widget/-/widget-1.0.0.tgz")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(tar.status(), StatusCode::OK);
    assert_eq!(body_bytes(tar.into_body()).await, tarball);

    // A publish routed to an upstream is rejected — a write can never land on
    // an upstream registry.
    let rejected = app
        .oneshot(
            Request::put("/lodash")
                .header("content-type", "application/json")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::from(json!({ "name": "lodash", "versions": {} }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn hosted_original_is_served_by_digest_after_restart_and_through_a_router() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.hosted.insert("acme".to_string(), hosted_with_access("acme", "$all"));
    let graph = vec![
        (
            "acme".to_string(),
            Registry::Hosted {
                patterns: vec![PackagePattern::parse("@acme/*", Ecosystem::Npm).unwrap()],
            },
        ),
        ("main".to_string(), Registry::Router { sources: vec!["acme".to_string()] }),
    ];
    config.registries = Registries::new(graph.into_iter().collect(), Some("main".to_string()));
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let tarball = b"public-hosted-original";
    let integrity_text = sha512_integrity(tarball);
    let integrity = integrity_text.parse().unwrap();
    let revision_path = integrity_addressed_tarball_path(&integrity).unwrap();
    let digest = revision_path.rsplit('/').next().unwrap();

    let publish = router_with_auth(config.clone(), auth.clone())
        .oneshot(hosted_publish_request(
            "/~acme/@acme/widget",
            "@acme/widget",
            "1.0.0",
            tarball,
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);

    let app = router_with_auth(config, auth);
    for path in [
        format!("/~acme/{revision_path}"),
        format!("/~main/{revision_path}"),
        format!("/{revision_path}"),
    ] {
        let response =
            app.clone().oneshot(Request::get(&path).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(response.headers().get(header::CACHE_CONTROL).unwrap(), "private, no-store");
        assert_eq!(response.headers().get(header::VARY).unwrap(), "Authorization");
        assert_eq!(
            response.headers().get(header::ETAG).unwrap().to_str().unwrap(),
            format!(r#""{digest}""#),
        );
        assert_eq!(
            response.headers().get("content-digest").unwrap().to_str().unwrap(),
            format!("sha-512=:{}:", integrity_text.strip_prefix("sha512-").unwrap()),
        );
        assert_eq!(body_bytes(response.into_body()).await, tarball, "{path}");
    }

    let canonical = app
        .oneshot(
            Request::get("/~acme/@acme/widget/-/widget-1.0.0.tgz").body(Body::empty()).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(canonical.status(), StatusCode::OK);
    assert_eq!(body_bytes(canonical.into_body()).await, tarball);
}

#[tokio::test]
async fn hosted_publish_rejects_digest_reference_overflow_without_disabling_existing_refs() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.hosted.insert("acme".to_string(), hosted_with_access("acme", "$authenticated"));
    config.registries = Registries::new(
        vec![("acme".to_string(), Registry::Hosted { patterns: vec![] })].into_iter().collect(),
        Some("acme".to_string()),
    );
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let tarball = b"shared-hosted-original";
    let integrity = sha512_integrity(tarball).parse().unwrap();
    let revision_path = integrity_addressed_tarball_path(&integrity).unwrap();
    let app = router_with_auth(config, auth);

    for index in 0..32 {
        let package = format!("shared-artifact-{index}");
        let response = app
            .clone()
            .oneshot(hosted_publish_request(
                &format!("/~acme/{package}"),
                &package,
                "1.0.0",
                tarball,
                &token,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED, "{package}");
    }

    let rejected = app
        .clone()
        .oneshot(hosted_publish_request(
            "/~acme/shared-artifact-overflow",
            "shared-artifact-overflow",
            "1.0.0",
            tarball,
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::CONFLICT);

    let overflow_packument = app
        .clone()
        .oneshot(
            Request::get("/~acme/shared-artifact-overflow")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(overflow_packument.status(), StatusCode::OK);
    let overflow_packument = body_json(overflow_packument.into_body()).await;
    assert!(overflow_packument["versions"].get("1.0.0").is_none());

    let existing = app
        .oneshot(
            Request::get(format!("/~acme/{revision_path}"))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(existing.status(), StatusCode::OK);
    assert_eq!(body_bytes(existing.into_body()).await, tarball);
}

#[tokio::test]
async fn hosted_digest_route_rechecks_package_access() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.hosted.insert("corp".to_string(), hosted_with_access("corp", "alice"));
    config.registries = Registries::new(
        vec![("corp".to_string(), Registry::Hosted { patterns: vec![] })].into_iter().collect(),
        Some("corp".to_string()),
    );
    let auth = AuthState::in_memory();
    let alice = auth.tokens.issue("alice").await.unwrap();
    let bob = auth.tokens.issue("bob").await.unwrap();
    let tarball = b"private-hosted-original";
    let integrity = sha512_integrity(tarball).parse().unwrap();
    let revision_path = integrity_addressed_tarball_path(&integrity).unwrap();
    let app = router_with_auth(config, auth);

    let publish = app
        .clone()
        .oneshot(hosted_publish_request("/~corp/secret", "secret", "1.0.0", tarball, &alice))
        .await
        .unwrap();
    assert_eq!(publish.status(), StatusCode::CREATED);

    for authorization in [None, Some(format!("Bearer {bob}"))] {
        let mut request = Request::get(format!("/~corp/{revision_path}"));
        if let Some(authorization) = authorization {
            request = request.header(header::AUTHORIZATION, authorization);
        }
        let response = app.clone().oneshot(request.body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.headers().get(header::CACHE_CONTROL).unwrap(), "private, no-store");
        assert_eq!(response.headers().get(header::VARY).unwrap(), "Authorization");
    }

    let response = app
        .oneshot(
            Request::get(format!("/~corp/{revision_path}"))
                .header(header::AUTHORIZATION, format!("Bearer {alice}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get(header::CACHE_CONTROL).unwrap(), "private, no-store");
    assert_eq!(response.headers().get(header::VARY).unwrap(), "Authorization");
    assert_eq!(body_bytes(response.into_body()).await, tarball);
}

/// A hosted registry's declared `patterns:` are enforced on the registry itself, on
/// every path to it: an off-pattern publish is rejected and an off-pattern
/// read is a definitive 404 — through a router and at the registry's own
/// `/~<name>/` URL alike — so a typo'd scope can never squat in the hosted
/// store and later surface as authoritative.
#[tokio::test]
async fn hosted_registry_patterns_bound_publish_and_reads_on_every_path() {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.hosted.insert("acme".to_string(), hosted_with_access("acme", "$authenticated"));
    // `acme` claims only `@acme/*`; the router has no other source, and the
    // path-less base aliases the router.
    let graph = vec![
        (
            "acme".to_string(),
            Registry::Hosted {
                patterns: vec![PackagePattern::parse("@acme/*", Ecosystem::Npm).unwrap()],
            },
        ),
        ("main".to_string(), Registry::Router { sources: vec!["acme".to_string()] }),
    ];
    let registries = Registries::new(graph.into_iter().collect(), Some("main".to_string()));
    registries.validate().expect("patterned hosted config is valid");
    config.registries = registries;
    let auth = AuthState::in_memory();
    let token = auth.tokens.issue("alice").await.unwrap();
    let app = router_with_auth(config, auth);

    let publish_body = |pkg: &str| {
        let tarball = b"typo-bytes";
        let bare = pkg.rsplit('/').next().unwrap();
        json!({
            "name": pkg,
            "dist-tags": { "latest": "1.0.0" },
            "versions": { "1.0.0": { "name": pkg, "version": "1.0.0", "dist": {
                "tarball": format!("http://example.test/{pkg}/-/{bare}-1.0.0.tgz"),
                "integrity": sha512_integrity(tarball),
            } } },
            "_attachments": { format!("{pkg}-1.0.0.tgz"): {
                "content_type": "application/octet-stream",
                "data": BASE64.encode(tarball),
                "length": tarball.len(),
            } },
        })
        .to_string()
    };
    let publish_to = |url: &str, pkg: &str| {
        Request::put(url)
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::from(publish_body(pkg)))
            .unwrap()
    };

    // An off-pattern publish is rejected on the registry's own URL, through the
    // router, and via the path-less base — and nothing lands in the store.
    for (url, pkg) in [
        ("/~acme/@typo/widget", "@typo/widget"),
        ("/~main/@typo/widget", "@typo/widget"),
        ("/@typo%2Fwidget", "@typo/widget"),
    ] {
        let rejected = app.clone().oneshot(publish_to(url, pkg)).await.unwrap();
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST, "publish {url} must be rejected");
    }
    assert!(
        !tmp.path().join("acme/@typo/widget/package.json").exists(),
        "an off-pattern publish must write nothing",
    );

    // A claimed name publishes normally through the registry's own URL.
    let accepted = app.clone().oneshot(publish_to("/~acme/@acme/widget", "@acme/widget")).await;
    assert_eq!(accepted.unwrap().status(), StatusCode::CREATED);

    // An off-pattern read is a definitive 404 on both addresses, before the
    // hosted store is consulted.
    for url in ["/~acme/@typo/widget", "/~main/@typo/widget"] {
        let read = app
            .clone()
            .oneshot(
                Request::get(url)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(read.status(), StatusCode::NOT_FOUND, "read {url} must 404");
    }
}

/// The off-pattern publish rejection explains a config fact about the
/// addressed registry, so it is only for callers the registry admits: a
/// denied caller gets the same 404 mask every read gives, and an unclaimed
/// probe cannot distinguish a private registry from an undefined one.
#[tokio::test]
async fn off_pattern_publish_is_masked_for_callers_the_registry_denies() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.hosted.insert("corp".to_string(), hosted_with_access("corp", "alice"));
    config.registries = Registries::new(
        vec![(
            "corp".to_string(),
            Registry::Hosted {
                patterns: vec![PackagePattern::parse("@corp/*", Ecosystem::Npm).unwrap()],
            },
        )]
        .into_iter()
        .collect(),
        None,
    );
    let auth = AuthState::in_memory();
    let member = auth.tokens.issue("alice").await.unwrap();
    let outsider = auth.tokens.issue("mallory").await.unwrap();
    let app = router_with_auth(config, auth);

    let publish_as = |token: &str| {
        Request::put("/~corp/@typo/widget")
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::from(json!({ "name": "@typo/widget", "versions": {} }).to_string()))
            .unwrap()
    };

    // The denied caller sees exactly what an undefined registry would give.
    let masked = app.clone().oneshot(publish_as(&outsider)).await.unwrap();
    assert_eq!(masked.status(), StatusCode::NOT_FOUND);

    // The member gets the loud off-pattern rejection.
    let rejected = app.oneshot(publish_as(&member)).await.unwrap();
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
}

/// The programmatic path enforces the same name/org safety as YAML loading:
/// a hosted `org` that could escape the storage root fails server startup.
#[tokio::test]
async fn building_the_server_rejects_an_unsafe_programmatic_hosted_org() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.hosted.insert("evil".to_string(), hosted_with_access("../escape", "$all"));
    let err = pnpr::try_router(config).expect_err("a path-escaping org must fail startup");
    assert!(err.to_string().contains("org"), "unexpected error: {err}");
}

/// The write endpoints gate on a private upstream's `access:` exactly as
/// reads do: a denied caller gets the read path's 403, not a 400 rejection
/// that narrates where the name routes.
#[tokio::test]
async fn publish_to_a_private_upstream_is_denied_before_the_upstream_rejection() {
    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    let mut corp = config.upstreams.get("npmjs").expect("default `npmjs` upstream").clone();
    corp.access = Some(AccessList::from_tokens(["alice"]));
    config.upstreams.insert("corp".to_string(), corp);
    let auth = AuthState::in_memory();
    let member = auth.tokens.issue("alice").await.unwrap();
    let outsider = auth.tokens.issue("mallory").await.unwrap();
    let app = router_with_auth(config, auth);

    let publish_as = |token: &str| {
        Request::put("/~corp/lodash")
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::from(json!({ "name": "lodash", "versions": {} }).to_string()))
            .unwrap()
    };

    let denied = app.clone().oneshot(publish_as(&outsider)).await.unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);

    // The admitted caller still can't write to an upstream, but the answer is
    // the clear rejection rather than an access denial.
    let rejected = app.oneshot(publish_as(&member)).await.unwrap();
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
}

/// The registry's access list gates writes exactly as it gates reads (RFC
/// "registries", implementation point 9). An authenticated non-member
/// passes the default per-package publish policy (`$authenticated`), so
/// without the registry-level gate they could publish into — or retag and
/// unpublish from — a private hosted org. The denial is the same 404 mask
/// reads use, so the write path reveals nothing about the registry either.
#[tokio::test]
async fn private_hosted_registry_denies_writes_from_non_members() {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    let tmp = TempDir::new().unwrap();
    let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
    config.hosted.insert("corp".to_string(), hosted_with_access("corp", "alice"));
    // `/~corp/` addresses the registry directly; the path-less base aliases it,
    // so the path-less dist-tag and unpublish writes route there too.
    config.registries = Registries::new(
        vec![("corp".to_string(), Registry::Hosted { patterns: vec![] })].into_iter().collect(),
        Some("corp".to_string()),
    );
    let auth = AuthState::in_memory();
    let member = auth.tokens.issue("alice").await.unwrap();
    let outsider = auth.tokens.issue("mallory").await.unwrap();
    let app = router_with_auth(config, auth);

    let tarball = b"corp-tool-1.0.0-bytes";
    let publish_body = json!({
        "name": "@corp/tool",
        "dist-tags": { "latest": "1.0.0" },
        "versions": { "1.0.0": { "name": "@corp/tool", "version": "1.0.0", "dist": {
            "tarball": "http://example.test/@corp/tool/-/tool-1.0.0.tgz",
            "integrity": sha512_integrity(tarball),
        } } },
        "_attachments": { "@corp/tool-1.0.0.tgz": {
            "content_type": "application/octet-stream",
            "data": BASE64.encode(tarball),
            "length": tarball.len(),
        } },
    })
    .to_string();
    let publish_as = |token: &str| {
        Request::put("/~corp/@corp/tool")
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::from(publish_body.clone()))
            .unwrap()
    };
    let retag_as = |token: &str| {
        Request::put("/-/package/@corp%2Ftool/dist-tags/latest")
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::from(r#""1.0.0""#))
            .unwrap()
    };

    // The non-member publish is masked with 404 — not 401/403, which would
    // reveal the private registry exists — and nothing lands on disk.
    let denied = app.clone().oneshot(publish_as(&outsider)).await.unwrap();
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
    assert!(
        !tmp.path().join("corp/@corp/tool/package.json").exists(),
        "a denied publish must write nothing",
    );

    // The member publishes normally.
    let allowed = app.clone().oneshot(publish_as(&member)).await.unwrap();
    assert_eq!(allowed.status(), StatusCode::CREATED);
    assert!(tmp.path().join("corp/@corp/tool/package.json").exists());

    // With the package now hosted, the non-member's path-less retag and
    // unpublish are masked the same way; the member's retag succeeds.
    let retag_denied = app.clone().oneshot(retag_as(&outsider)).await.unwrap();
    assert_eq!(retag_denied.status(), StatusCode::NOT_FOUND);
    let unpublish_denied = app
        .clone()
        .oneshot(
            Request::delete("/@corp%2Ftool/-rev/1")
                .header(header::AUTHORIZATION, format!("Bearer {outsider}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unpublish_denied.status(), StatusCode::NOT_FOUND);
    assert!(
        tmp.path().join("corp/@corp/tool/package.json").exists(),
        "a denied unpublish must remove nothing",
    );
    let retag_allowed = app.oneshot(retag_as(&member)).await.unwrap();
    assert_eq!(retag_allowed.status(), StatusCode::CREATED);
}

/// Search scans the flat hosted store, so it must apply the owning registry's
/// access list, not only the per-package ACL: a private flat-root registry
/// (`org: ""`) is 404-masked on packument reads and must not be enumerable
/// by name/version/description through `/-/v1/search`.
#[tokio::test]
async fn search_does_not_enumerate_a_private_flat_root_registry() {
    let search_with = |url: &str, authorization: Option<String>| {
        let mut request = Request::get(url);
        if let Some(value) = authorization {
            request = request.header(header::AUTHORIZATION, value);
        }
        request.body(Body::empty()).unwrap()
    };

    for url in ["/-/v1/search?text=secret", "/-/v1/search?browse=true"] {
        let tmp = TempDir::new().unwrap();
        seed_hosted(tmp.path(), "@corp/secret-tool");

        let mut config = config_for("http://127.0.0.1:1", tmp.path().to_path_buf());
        // A surviving public flat-root registry would defeat this access check.
        config.hosted.clear();
        config.hosted.insert("corp".to_string(), hosted_with_access("", "alice"));
        config.registries = Registries::new(
            vec![("corp".to_string(), Registry::Hosted { patterns: vec![] })].into_iter().collect(),
            Some("corp".to_string()),
        );
        let auth = AuthState::in_memory();
        let member = auth.tokens.issue("alice").await.unwrap();
        let outsider = auth.tokens.issue("mallory").await.unwrap();
        let app = router_with_auth(config, auth);

        for authorization in [None, Some(format!("Bearer {outsider}"))] {
            let response = app.clone().oneshot(search_with(url, authorization)).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = body_json(response.into_body()).await;
            assert_eq!(body["total"], json!(0), "private package leaked through search");
            assert_eq!(body["objects"], json!([]));
        }

        let response =
            app.oneshot(search_with(url, Some(format!("Bearer {member}")))).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response.into_body()).await;
        assert_eq!(body["objects"][0]["package"]["name"], json!("@corp/secret-tool"));
    }
}
