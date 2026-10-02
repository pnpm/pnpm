use super::Request;
use crate::{
    Client, Method,
    header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HeaderName},
};

#[test]
fn cross_origin_redirect_drops_otp_and_auth() {
    let mut request = Client::new()
        .post("https://registry.example/login")
        .header(AUTHORIZATION, "Basic secret")
        .header("npm-otp", "123456")
        .build()
        .expect("request");

    request
        .follow_redirect("https://other.example/login".parse().unwrap(), 302)
        .unwrap();

    assert_eq!(request.method(), Method::GET);
    assert!(!request.headers().contains_key(AUTHORIZATION));
    assert!(
        !request
            .headers()
            .contains_key(HeaderName::from_static("npm-otp"))
    );
}

#[test]
fn cross_origin_body_preserving_redirect_is_rejected() {
    for (method, status) in
        [(Method::PUT, 301), (Method::PUT, 302), (Method::POST, 307), (Method::POST, 308)]
    {
        let mut request = Client::new()
            .request(method, "https://registry.example/login")
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_LENGTH, "21")
            .body(r#"{"password":"secret"}"#)
            .build()
            .expect("request");

        let error = request
            .follow_redirect("https://other.example/login".parse().unwrap(), status)
            .expect_err("credential body cannot cross origins");

        assert!(error.is_redirect());
        assert_eq!(request.url().as_str(), "https://registry.example/login");
    }
}

#[test]
fn same_origin_body_preserving_redirect_keeps_request() {
    let mut request: Request = Client::new()
        .put("https://registry.example/login")
        .header("npm-otp", "123456")
        .body("payload")
        .build()
        .expect("request");

    request
        .follow_redirect("https://registry.example/new-login".parse().unwrap(), 307)
        .unwrap();

    assert_eq!(request.method(), Method::PUT);
    assert_eq!(request.body.as_deref(), Some("payload".as_bytes()));
    assert_eq!(request.headers()["npm-otp"], "123456");
}
