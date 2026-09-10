use super::{
    AccessList, AuthState, Body, Ecosystem, PackagePattern, PackageRule, PackageRules, Request,
    ServiceExt, StatusCode, TempDir, Value, app, basic, body_bytes, get, header, json, oci_config,
    push_image, router_with_auth, token,
};

#[tokio::test]
async fn the_catalog_lists_only_hosted_repositories() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;
    push_image(&app, &auth, "acme/team/tool", "1.0").await;

    let response = get(&app, "/v2/_catalog").await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["repositories"], json!(["acme/app", "acme/team/tool"]));
}

#[tokio::test]
async fn a_name_no_hosted_registry_claims_is_not_served() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    // `other/app` routes to the image upstream, which is not proxied yet.
    let request = Request::post("/v2/other/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    assert_eq!(get(&app, "/v2/other/app/tags/list").await.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_catalog_omits_repositories_the_caller_may_not_read() {
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    let hosted = config.hosted.get_mut("images").expect("the hosted image registry");
    // Reads are open by default, and `acme/secret` refines that to require a
    // caller. A listing must apply the same rule its fetches would.
    hosted.rules = PackageRules::new(
        vec![PackageRule {
            pattern: PackagePattern::parse("acme/secret", Ecosystem::Oci).unwrap(),
            access: Some(AccessList::from_tokens(["$authenticated"])),
            publish: None,
            unpublish: None,
        }],
        Some(AccessList::from_tokens(["$all"])),
    );
    let app = router_with_auth(config, AuthState::in_memory());
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;
    push_image(&app, &auth, "acme/secret", "1.0").await;

    let response = get(&app, "/v2/_catalog").await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["repositories"], json!(["acme/app"]));

    let request = Request::get("/v2/_catalog")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["repositories"], json!(["acme/app", "acme/secret"]));
}
