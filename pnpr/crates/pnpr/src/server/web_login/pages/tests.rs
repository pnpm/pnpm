use axum::{http::StatusCode, response::Response};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use sha2::{Digest as _, Sha256};

use super::{code_page, provider_page};

async fn body(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn the_policy_allows_exactly_the_pages_style() {
    let response = code_page(StatusCode::OK, None);
    let policy = response.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .to_string();
    let html = body(response).await;
    let style = html
        .split_once("<style>")
        .and_then(|(_, rest)| rest.split_once("</style>"))
        .unwrap()
        .0;
    let hash = BASE64_STANDARD.encode(Sha256::digest(style.as_bytes()));
    assert_eq!(
        policy,
        format!("default-src 'none'; style-src 'sha256-{hash}'; frame-ancestors 'none'"),
    );
}

#[tokio::test]
async fn an_error_shows_on_the_page_with_its_status() {
    let response = code_page(StatusCode::BAD_GATEWAY, Some("Provider <down>"));
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    let html = body(response).await;
    assert!(html.contains(r#"<p class="error" role="alert">Provider &lt;down&gt;</p>"#), "{html}");
}

#[tokio::test]
async fn provider_names_and_the_code_are_escaped() {
    let provider: pnpr_config::oidc::OidcProvider = serde_json::from_value(serde_json::json!({
        "name": r#"a"b"#,
        "issuer": "https://login.example",
        "audience": "pnpr",
        "login": { "users": [{ "subject": "alice", "userId": "alice" }] },
    }))
    .unwrap();
    let html = body(provider_page(StatusCode::OK, &[&provider], "<x>", None)).await;
    assert!(html.contains(r#"value="a&quot;b">Sign in with a&quot;b</button>"#), "{html}");
    assert!(html.contains(r#"name="code" value="&lt;x&gt;""#), "{html}");
}
