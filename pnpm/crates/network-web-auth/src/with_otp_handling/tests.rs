use super::{OtpChallenge, OtpErrorBody, otp_challenge_from_unauthorized_body};

#[test]
fn a_body_with_both_web_auth_urls_is_a_web_challenge() {
    let body = br#"{"error":"one-time pass required","authUrl":"https://auth.example/login","doneUrl":"https://auth.example/done"}"#;
    assert_eq!(
        otp_challenge_from_unauthorized_body(body),
        Some(OtpChallenge {
            body: Some(OtpErrorBody {
                auth_url: Some("https://auth.example/login".to_owned()),
                done_url: Some("https://auth.example/done".to_owned()),
            }),
        }),
    );
}

/// Both keys present but not both strings: still a challenge, so the flow
/// falls back to the classic prompt instead of failing as unauthorized.
#[test]
fn non_string_web_auth_urls_are_dropped_from_the_challenge() {
    let body = br#"{"authUrl":42,"doneUrl":"https://auth.example/done"}"#;
    assert_eq!(
        otp_challenge_from_unauthorized_body(body),
        Some(OtpChallenge {
            body: Some(OtpErrorBody {
                auth_url: None,
                done_url: Some("https://auth.example/done".to_owned()),
            }),
        }),
    );
}

#[test]
fn one_of_the_two_urls_is_not_a_web_challenge() {
    let body = br#"{"authUrl":"https://auth.example/login"}"#;
    assert_eq!(otp_challenge_from_unauthorized_body(body), None);
}

#[test]
fn the_classic_wording_is_a_challenge_without_a_body() {
    let body = br#"{"error":"You must provide a One-Time Pass. Upgrade your client to npm@latest in order to use 2FA."}"#;
    assert_eq!(otp_challenge_from_unauthorized_body(body), Some(OtpChallenge { body: None }));
    assert_eq!(
        otp_challenge_from_unauthorized_body(b"one-time pass"),
        Some(OtpChallenge { body: None }),
        "the wording is recognized outside JSON too",
    );
}

#[test]
fn a_plain_unauthorized_body_is_no_challenge() {
    assert_eq!(otp_challenge_from_unauthorized_body(br#"{"error":"unauthorized"}"#), None);
    assert_eq!(otp_challenge_from_unauthorized_body(b"Bad token"), None);
    assert_eq!(otp_challenge_from_unauthorized_body(b""), None);
}
