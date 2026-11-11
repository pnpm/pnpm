use axum::http::{HeaderMap, HeaderValue};

use super::is_same_origin_request;

fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in pairs {
        headers.insert(*name, HeaderValue::from_static(value));
    }
    headers
}

#[test]
fn only_a_same_origin_form_submission_confirms_a_sign_in() {
    let accepted = [
        headers(&[("sec-fetch-site", "same-origin")]),
        headers(&[("sec-fetch-site", "same-origin"), ("origin", "null")]),
    ];
    for request in accepted {
        assert!(is_same_origin_request(&request), "{request:?}");
    }
    let refused = [
        HeaderMap::new(),
        headers(&[("sec-fetch-site", "cross-site")]),
        headers(&[("sec-fetch-site", "same-site")]),
        headers(&[("sec-fetch-site", "none")]),
        headers(&[("origin", "https://registry.example")]),
        headers(&[("origin", "null")]),
    ];
    for request in refused {
        assert!(!is_same_origin_request(&request), "{request:?}");
    }
}
