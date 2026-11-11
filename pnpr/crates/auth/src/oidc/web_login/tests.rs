use super::{OidcState, USER_CODE_ALPHABET, WEB_LOGIN_TTL, WebLoginPoll, WebLoginState};
use chrono::Utc;

fn oidc() -> OidcState {
    OidcState::new(&[], "https://registry.example").unwrap()
}

#[tokio::test]
async fn user_codes_use_the_documented_shape() {
    let code = oidc().create_web_login().expect("session created").user_code;
    let groups: Vec<&str> = code.split('-').collect();
    assert_eq!(groups.len(), 2, "{code}");
    for group in groups {
        assert_eq!(group.len(), 4, "{code}");
        assert!(
            group
                .bytes()
                .all(|byte| USER_CODE_ALPHABET.contains(&byte)),
            "{code}",
        );
    }
}

#[tokio::test]
async fn a_login_is_pending_until_claimed_and_fulfilled() {
    let state = oidc();
    let tokens = state.create_web_login().expect("session created");
    assert_eq!(state.poll_web_login(&tokens.poll_token), WebLoginPoll::Pending);

    let login_id = state.claim_web_login(&tokens.user_code).expect("claimed");
    assert_eq!(state.poll_web_login(&tokens.poll_token), WebLoginPoll::Pending);

    state.fulfill_web_login(&login_id, "pnpr_oidc_secret_token".to_string()).expect("fulfilled");
    assert_eq!(
        state.poll_web_login(&tokens.poll_token),
        WebLoginPoll::Ready("pnpr_oidc_secret_token".to_string()),
    );
    assert_eq!(state.poll_web_login(&tokens.poll_token), WebLoginPoll::NotFound);
}

#[tokio::test]
async fn polling_needs_the_poll_token() {
    let state = oidc();
    let tokens = state.create_web_login().expect("session created");
    assert_eq!(state.poll_web_login(&tokens.user_code), WebLoginPoll::NotFound);
    assert_eq!(state.poll_web_login("unknown"), WebLoginPoll::NotFound);
    let login_id = state.claim_web_login(&tokens.user_code).expect("claimed");
    assert_eq!(state.poll_web_login(&login_id), WebLoginPoll::NotFound);
}

#[tokio::test]
async fn a_user_code_is_claimed_once_in_any_spelling() {
    let state = oidc();
    let code = state.create_web_login().expect("session created").user_code;
    let typed = format!(" {} ", code.to_lowercase().replace('-', " "));
    assert!(state.claim_web_login(&typed).is_some());
    assert!(state.claim_web_login(&code).is_none());
}

#[tokio::test]
async fn unknown_and_malformed_user_codes_claim_nothing() {
    let state = oidc();
    state.create_web_login().expect("session created");
    for code in ["", "BCDF-GHJ", "BCDF-GHJKL", "AEIO-UAEI", "BCDF-GHJK"] {
        assert!(state.claim_web_login(code).is_none(), "{code}");
    }
}

#[tokio::test]
async fn only_a_claimed_login_can_be_fulfilled_and_only_once() {
    let state = oidc();
    let tokens = state.create_web_login().expect("session created");
    assert!(state.fulfill_web_login("unknown", "token".to_string()).is_err());
    let login_id = state.claim_web_login(&tokens.user_code).expect("claimed");
    assert!(state.fulfill_web_login(&login_id, "token1".to_string()).is_ok());
    assert!(state.fulfill_web_login(&login_id, "token2".to_string()).is_err());
}

#[tokio::test]
async fn an_expired_login_cannot_be_claimed_fulfilled_or_polled() {
    let state = oidc();
    let tokens = state.create_web_login().expect("session created");
    let expire = || {
        let mut logins = state.browser.web_logins.lock().unwrap();
        for entry in logins.entries.values_mut() {
            entry.expires = Utc::now().timestamp() - 1;
        }
    };
    let claimed = state.create_web_login().expect("session created");
    let login_id = state.claim_web_login(&claimed.user_code).expect("claimed");
    expire();

    assert!(state.claim_web_login(&tokens.user_code).is_none());
    assert!(state.fulfill_web_login(&login_id, "token".to_string()).is_err());
    assert_eq!(state.poll_web_login(&tokens.poll_token), WebLoginPoll::NotFound);
}

#[tokio::test]
async fn max_entries_enforced() {
    let state = oidc();
    let now = Utc::now().timestamp();
    {
        let mut logins = state.browser.web_logins.lock().unwrap();
        for index in 0..1024 {
            logins.entries.insert(
                format!("login_{index}"),
                super::WebLoginEntry {
                    state: WebLoginState::Pending,
                    expires: now + WEB_LOGIN_TTL.as_secs() as i64,
                    poll_hash: format!("poll_hash_{index}"),
                    code_hash: format!("code_hash_{index}"),
                },
            );
        }
    }
    assert!(state.create_web_login().is_err());
}
