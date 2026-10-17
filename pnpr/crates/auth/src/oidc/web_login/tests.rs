use super::{OidcState, WEB_LOGIN_TTL, WebLoginPoll, WebLoginState};
use chrono::Utc;

#[tokio::test]
async fn create_and_poll_pending_web_login() {
    let state = OidcState::new(&[], "https://registry.example").unwrap();
    let tokens = state.create_web_login().expect("session created");
    assert!(!tokens.browser_session_id.is_empty());
    assert!(!tokens.poll_token.is_empty());
    assert_ne!(tokens.browser_session_id, tokens.poll_token);

    assert!(state.is_valid_web_login(&tokens.browser_session_id));
    assert!(!state.is_valid_web_login(&tokens.poll_token));
    assert!(!state.is_valid_web_login("unknown_session"));

    assert_eq!(state.poll_web_login(&tokens.poll_token), WebLoginPoll::Pending);
    // Polling with browser session id must not work
    assert_eq!(state.poll_web_login(&tokens.browser_session_id), WebLoginPoll::NotFound);
    assert_eq!(state.poll_web_login("unknown_session"), WebLoginPoll::NotFound);
}

#[tokio::test]
async fn fulfill_and_poll_ready_consumes_session() {
    let state = OidcState::new(&[], "https://registry.example").unwrap();
    let tokens = state.create_web_login().expect("session created");

    state
        .fulfill_web_login(&tokens.browser_session_id, "pnpr_oidc_secret_token".to_string())
        .expect("fulfilled");

    // Once fulfilled, it is no longer pending validation
    assert!(!state.is_valid_web_login(&tokens.browser_session_id));

    // First poll consumes the session and returns Ready
    assert_eq!(
        state.poll_web_login(&tokens.poll_token),
        WebLoginPoll::Ready("pnpr_oidc_secret_token".to_string()),
    );

    // Second poll returns NotFound because it was consumed
    assert_eq!(state.poll_web_login(&tokens.poll_token), WebLoginPoll::NotFound);
}

#[tokio::test]
async fn fulfill_fails_on_unknown_or_already_fulfilled_session() {
    let state = OidcState::new(&[], "https://registry.example").unwrap();
    assert!(state.fulfill_web_login("unknown", "token".to_string()).is_err());

    let tokens = state.create_web_login().expect("session created");
    assert!(state.fulfill_web_login(&tokens.browser_session_id, "token1".to_string()).is_ok());
    // Second fulfillment attempt fails
    assert!(state.fulfill_web_login(&tokens.browser_session_id, "token2".to_string()).is_err());
}

#[tokio::test]
async fn expired_session_fails_validation_and_polling() {
    let state = OidcState::new(&[], "https://registry.example").unwrap();
    let tokens = state.create_web_login().expect("session created");

    // Manually expire the session entry
    let hash = crate::sha256_hex(tokens.browser_session_id.as_bytes());
    {
        let mut logins = state.browser.web_logins.lock().unwrap();
        if let Some(entry) = logins.entries.get_mut(&hash) {
            entry.expires = Utc::now().timestamp() - 1;
        }
    }

    assert!(!state.is_valid_web_login(&tokens.browser_session_id));
    assert!(state.fulfill_web_login(&tokens.browser_session_id, "token".to_string()).is_err());
    assert_eq!(state.poll_web_login(&tokens.poll_token), WebLoginPoll::NotFound);
}

#[tokio::test]
async fn max_entries_enforced() {
    let state = OidcState::new(&[], "https://registry.example").unwrap();
    let now = Utc::now().timestamp();
    {
        let mut logins = state.browser.web_logins.lock().unwrap();
        for index in 0..1024 {
            let browser_hash = format!("browser_hash_{index}");
            let poll_hash = format!("poll_hash_{index}");
            logins.entries.insert(
                browser_hash.clone(),
                super::WebLoginEntry {
                    state: WebLoginState::Pending,
                    expires: now + WEB_LOGIN_TTL.as_secs() as i64,
                    poll_hash: poll_hash.clone(),
                },
            );
            logins.poll_to_browser.insert(poll_hash, browser_hash);
        }
    }

    // Creating another session when full returns unavailable
    assert!(state.create_web_login().is_err());
}
