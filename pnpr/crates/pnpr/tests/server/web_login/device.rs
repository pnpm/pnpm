//! The browser side of the CLI web login: the code page, the provider
//! choice, and what happens when a code is submitted.

use super::{
    Body, Request, ServiceExt, StatusCode, TempDir, config_with_multiple_login_providers,
    config_with_single_login_provider, config_without_oidc, header, html, router, start_login_at,
    submit_code,
};

#[tokio::test]
async fn the_device_page_asks_only_for_the_code() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_multiple_login_providers(&storage));
    let response = app
        .oneshot(
            Request::get("/-/v1/login/device")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "text/html; charset=utf-8");
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");

    let html = html(response).await;
    assert!(html.contains(r#"<input id="code" name="code""#), "{html}");
    assert_eq!(html.matches("<button").count(), 1, "{html}");
    assert!(html.contains(r#"<button type="submit">Continue</button>"#), "{html}");
}

#[tokio::test]
async fn with_several_providers_an_accepted_code_leads_to_the_provider_choice() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_multiple_login_providers(&storage));
    let (code, _, _) = start_login_at(&app, "/-/v1/login").await;

    let wrong = app
        .clone()
        .oneshot(submit_code("BCDF-GHJK", None, Some("same-origin")))
        .await
        .unwrap();
    assert_eq!(wrong.status(), StatusCode::BAD_REQUEST);
    assert!(html(wrong).await.contains("That code does not match a pending sign-in."));

    let accepted = app
        .clone()
        .oneshot(submit_code(&code.to_lowercase(), None, Some("same-origin")))
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::OK);
    assert!(!accepted.headers().contains_key(header::SET_COOKIE));
    let html = html(accepted).await;
    assert!(html.contains("Choose where you sign in."), "{html}");
    assert!(html.contains(r#"name="provider" value="google">Sign in with google<"#), "{html}");
    assert!(html.contains(r#"name="provider" value="okta">Sign in with okta<"#), "{html}");
    assert!(html.contains(&format!(r#"name="code" value="{}""#, code.to_lowercase())), "{html}");

    let again = app
        .oneshot(submit_code(&code, None, Some("same-origin")))
        .await
        .unwrap();
    assert_eq!(again.status(), StatusCode::OK, "checking the code must not claim it");
}

#[tokio::test]
async fn the_device_page_is_absent_without_oidc_login() {
    let storage = TempDir::new().unwrap();
    let app = router(config_without_oidc(&storage));
    for request in [
        Request::get("/-/v1/login/device")
            .body(Body::empty())
            .unwrap(),
        submit_code("BCDF-GHJK", Some("company"), Some("same-origin")),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request)
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND,
        );
    }
}

#[tokio::test]
async fn a_cli_login_cannot_be_completed_without_the_typed_code() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_single_login_provider(&storage));

    let direct = app
        .clone()
        .oneshot(
            Request::get("/-/oidc/company/login?return=cli")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(direct.status(), StatusCode::BAD_REQUEST);

    let wrong = app
        .oneshot(submit_code("BCDF-GHJK", Some("company"), Some("same-origin")))
        .await
        .unwrap();
    assert_eq!(wrong.status(), StatusCode::BAD_REQUEST);
    assert!(!wrong.headers().contains_key(header::SET_COOKIE));
    let html = html(wrong).await;
    assert!(html.contains("That code does not match a pending sign-in."), "{html}");
}

#[tokio::test]
async fn the_code_is_accepted_only_from_a_same_origin_form_for_a_known_provider() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_single_login_provider(&storage));
    let (code, _, _) = start_login_at(&app, "/-/v1/login").await;
    for request in [
        submit_code(&code, Some("company"), None),
        submit_code(&code, Some("company"), Some("cross-site")),
        submit_code(&code, Some("company"), Some("same-site")),
        submit_code(&code, Some("github"), Some("same-origin")),
        submit_code(&code, Some("unknown"), Some("same-origin")),
    ] {
        let response = app
            .clone()
            .oneshot(request)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(!response.headers().contains_key(header::SET_COOKIE));
    }
}

#[tokio::test]
async fn code_guesses_are_rate_limited() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_single_login_provider(&storage));
    for _ in 0..20 {
        let response = app
            .clone()
            .oneshot(submit_code("BCDF-GHJK", Some("company"), Some("same-origin")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    let throttled = app
        .oneshot(submit_code("BCDF-GHJK", Some("company"), Some("same-origin")))
        .await
        .unwrap();
    assert_eq!(throttled.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn a_sign_in_that_cannot_start_leaves_the_code_usable() {
    let storage = TempDir::new().unwrap();
    let app = router(config_with_single_login_provider(&storage));
    let (code, _, _) = start_login_at(&app, "/-/v1/login").await;
    for provider in [None, Some("company")] {
        let response = app
            .clone()
            .oneshot(submit_code(&code, provider, Some("same-origin")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert!(!response.headers().contains_key(header::SET_COOKIE));
        let html = html(response).await;
        assert!(html.contains("The sign-in provider is not reachable."), "{html}");
    }
}
