use super::{OidcState, WEB_LOGIN_TTL, WebLoginPoll, WebLoginState};
use chrono::Utc;

#[tokio::test]
async fn create_and_poll_pending_web_login() {
    let state = OidcState::new(&[], "https://registry.example").unwrap();
    let session_id = state.create_web_login().expect("session created");
    assert!(!session_id.is_empty());
    assert!(state.is_valid_web_login(&session_id));
    assert!(!state.is_valid_web_login("unknown_session"));

    assert_eq!(state.poll_web_login(&session_id), WebLoginPoll::Pending);
    assert_eq!(state.poll_web_login("unknown_session"), WebLoginPoll::NotFound);
}

#[tokio::test]
async fn fulfill_and_poll_ready_consumes_session() {
    let state = OidcState::new(&[], "https://registry.example").unwrap();
    let session_id = state.create_web_login().expect("session created");

    state.fulfill_web_login(&session_id, "pnpr_oidc_secret_token".to_string()).expect("fulfilled");

    // Once fulfilled, it is no longer pending validation
    assert!(!state.is_valid_web_login(&session_id));

    // First poll consumes the session and returns Ready
    assert_eq!(
        state.poll_web_login(&session_id),
        WebLoginPoll::Ready("pnpr_oidc_secret_token".to_string()),
    );

    // Second poll returns NotFound because it was consumed
    assert_eq!(state.poll_web_login(&session_id), WebLoginPoll::NotFound);
}

#[tokio::test]
async fn fulfill_fails_on_unknown_or_already_fulfilled_session() {
    let state = OidcState::new(&[], "https://registry.example").unwrap();
    assert!(state.fulfill_web_login("unknown", "token".to_string()).is_err());

    let session_id = state.create_web_login().expect("session created");
    assert!(state.fulfill_web_login(&session_id, "token1".to_string()).is_ok());
    // Second fulfillment attempt fails
    assert!(state.fulfill_web_login(&session_id, "token2".to_string()).is_err());
}

#[tokio::test]
async fn expired_session_fails_validation_and_polling() {
    let state = OidcState::new(&[], "https://registry.example").unwrap();
    let session_id = state.create_web_login().expect("session created");

    // Manually expire the session entry
    let hash = crate::sha256_hex(session_id.as_bytes());
    {
        let mut logins = state.browser.web_logins.lock().unwrap();
        if let Some(entry) = logins.get_mut(&hash) {
            entry.expires = Utc::now().timestamp() - 1;
        }
    }

    assert!(!state.is_valid_web_login(&session_id));
    assert!(state.fulfill_web_login(&session_id, "token".to_string()).is_err());
    assert_eq!(state.poll_web_login(&session_id), WebLoginPoll::NotFound);
}

#[tokio::test]
async fn max_entries_enforced() {
    let state = OidcState::new(&[], "https://registry.example").unwrap();
    let now = Utc::now().timestamp();
    {
        let mut logins = state.browser.web_logins.lock().unwrap();
        for index in 0..1024 {
            logins.insert(
                format!("hash_{index}"),
                super::WebLoginEntry {
                    state: WebLoginState::Pending,
                    expires: now + WEB_LOGIN_TTL.as_secs() as i64,
                },
            );
        }
    }

    // Creating another session when full returns unavailable
    assert!(state.create_web_login().is_err());
}
