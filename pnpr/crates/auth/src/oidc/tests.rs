use super::{
    OidcState, token_payload, verify_workload,
    workload::{binding_matches, validate_times},
};
use axum::{
    Json, Router,
    extract::{Form, State},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as BASE64_URL_SAFE_NO_PAD};
use chrono::Utc;
use openidconnect::core::CoreProviderMetadata;
use p256::ecdsa::{Signature, SigningKey, signature::Signer as _};
use pnpr_config::oidc::{OidcBinding, OidcLogin, OidcProvider, OidcWorkload};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use url::Url;

fn signing_key() -> SigningKey {
    SigningKey::from_bytes((&[42u8; 32]).into()).unwrap()
}

fn jwks(key: &SigningKey) -> Value {
    let point = key.verifying_key().to_encoded_point(false);
    json!({"keys": [{"kty": "EC", "crv": "P-256", "alg": "ES256", "use": "sig", "kid": "test",
        "x": BASE64_URL_SAFE_NO_PAD.encode(point.x().unwrap()),
        "y": BASE64_URL_SAFE_NO_PAD.encode(point.y().unwrap())}]})
}

fn sign(payload: &Value, key: &SigningKey) -> String {
    let header = BASE64_URL_SAFE_NO_PAD.encode(br#"{"alg":"ES256","kid":"test"}"#);
    let payload = BASE64_URL_SAFE_NO_PAD.encode(serde_json::to_vec(payload).unwrap());
    let message = format!("{header}.{payload}");
    let signature: Signature = key.sign(message.as_bytes());
    format!("{message}.{}", BASE64_URL_SAFE_NO_PAD.encode(signature.to_bytes()))
}

fn config(issuer: &str) -> OidcProvider {
    OidcProvider {
        name: "example".to_string(),
        issuer: issuer.to_string(),
        audience: "pnpr-ci".to_string(),
        login: None,
        workloads: vec![OidcWorkload {
            identity: OidcBinding {
                subject: "repo:org/repo:ref:refs/heads/main".to_string(),
                username: "ci".to_string(),
                claims: BTreeMap::from([("repository_id".to_string(), "123".to_string())]),
            },
            registry: "private".to_string(),
            packages: vec!["@org/pkg".to_string()],
        }],
    }
}

fn payload(issuer: &str) -> Value {
    let now = Utc::now().timestamp();
    json!({"iss": issuer, "aud": "pnpr-ci", "sub": "repo:org/repo:ref:refs/heads/main",
        "repository_id": "123", "iat": now, "nbf": now, "exp": now + 300})
}

fn metadata(issuer: &str) -> Value {
    json!({"issuer": issuer, "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"), "jwks_uri": format!("{issuer}/jwks"),
        "response_types_supported": ["code"], "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["ES256"],
        "token_endpoint_auth_methods_supported": ["client_secret_basic"]})
}

#[test]
fn verifies_signature_issuer_audience_and_times() {
    let issuer = "https://issuer.example";
    let config = config(issuer);
    let metadata: CoreProviderMetadata = serde_json::from_value(metadata(issuer)).unwrap();
    let metadata = metadata.set_jwks(serde_json::from_value(jwks(&signing_key())).unwrap());
    verify_workload(&config, &metadata, &sign(&payload(issuer), &signing_key())).unwrap();
    for (claim, value) in [
        ("iss", json!("https://attacker.example")),
        ("aud", json!("other")),
        ("exp", json!(0)),
        ("nbf", json!(Utc::now().timestamp() + 100)),
        ("iat", json!(Utc::now().timestamp() + 100)),
        ("nbf", json!("tomorrow")),
        ("azp", json!("another-client")),
    ] {
        let mut claims = payload(issuer);
        claims[claim] = value;
        assert!(
            verify_workload(&config, &metadata, &sign(&claims, &signing_key())).is_err(),
            "accepted {claim}: {claims}",
        );
    }
    let other_key = SigningKey::from_bytes((&[43u8; 32]).into()).unwrap();
    assert!(verify_workload(&config, &metadata, &sign(&payload(issuer), &other_key)).is_err());
    let unsigned = format!(
        "{}.{}.",
        BASE64_URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#),
        BASE64_URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload(issuer)).unwrap()),
    );
    assert!(verify_workload(&config, &metadata, &unsigned).is_err());
    for missing in ["iss", "aud", "sub", "exp", "iat"] {
        let mut claims = payload(issuer);
        claims.as_object_mut().unwrap().remove(missing);
        assert!(
            verify_workload(&config, &metadata, &sign(&claims, &signing_key())).is_err(),
            "accepted missing {missing}",
        );
    }
}

#[test]
fn bindings_require_exact_subject_and_string_claims() {
    let config = config("https://issuer.example");
    let binding = &config.workloads[0].identity;
    let mut claims = payload(&config.issuer);
    assert!(binding_matches(binding, &claims));
    claims["sub"] = json!("repo:org/repo:pull_request");
    assert!(!binding_matches(binding, &claims));
    claims["sub"] = json!(binding.subject);
    for value in [json!(123), json!("124"), Value::Null, json!(["123"])] {
        claims["repository_id"] = value;
        assert!(!binding_matches(binding, &claims), "accepted {claims}");
    }
}

struct MockProvider {
    issuer: String,
    key: SigningKey,
    nonce: Mutex<Option<String>>,
    challenge: Mutex<Option<String>>,
    key_requests: Mutex<usize>,
    token_requests: Mutex<usize>,
    auth_method: Mutex<String>,
    delay_discovery: std::sync::atomic::AtomicBool,
    discovery_started: tokio::sync::Notify,
    release_discovery: tokio::sync::Notify,
}

async fn mock_provider() -> (Arc<MockProvider>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let state = Arc::new(MockProvider {
        issuer: format!("http://{}", listener.local_addr().unwrap()),
        key: signing_key(),
        nonce: Mutex::new(None),
        challenge: Mutex::new(None),
        key_requests: Mutex::new(0),
        token_requests: Mutex::new(0),
        auth_method: Mutex::new("client_secret_basic".to_string()),
        delay_discovery: std::sync::atomic::AtomicBool::new(false),
        discovery_started: tokio::sync::Notify::new(),
        release_discovery: tokio::sync::Notify::new(),
    });
    let router = Router::new()
        .route("/.well-known/openid-configuration", get(async |State(state): State<Arc<MockProvider>>| {
                if state.delay_discovery.load(std::sync::atomic::Ordering::Relaxed) {
                    state.discovery_started.notify_one();
                    state.release_discovery.notified().await;
                }
                let mut document = metadata(&state.issuer);
                document["token_endpoint_auth_methods_supported"] = json!([*state.auth_method.lock().unwrap()]);
                Json(document)
            }))
        .route("/jwks", get(async |State(state): State<Arc<MockProvider>>| {
            *state.key_requests.lock().unwrap() += 1;
            Json(jwks(&state.key))
        }))
        .route("/token", post(async |State(state): State<Arc<MockProvider>>, Form(form): Form<HashMap<String, String>>| {
            *state.token_requests.lock().unwrap() += 1;
            if form.get("code").is_some_and(|code| code == "invalid") {
                return Json(json!({"error": "invalid_grant"}));
            }
            assert_eq!(form.get("grant_type").unwrap(), "authorization_code");
                if *state.auth_method.lock().unwrap() == "client_secret_post" {
                    assert_eq!(form.get("client_secret").unwrap(), "secret");
                    assert_eq!(form.get("client_id").unwrap(), "pnpr-ci");
                }
            let challenge = BASE64_URL_SAFE_NO_PAD.encode(Sha256::digest(form.get("code_verifier").unwrap().as_bytes()));
            assert_eq!(Some(challenge), *state.challenge.lock().unwrap());
            let mut claims = payload(&state.issuer);
            claims["nonce"] = json!(*state.nonce.lock().unwrap());
            Json(json!({"access_token": "access", "token_type": "Bearer", "id_token": sign(&claims, &state.key)}))
        })).with_state(Arc::clone(&state));
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (state, task)
}

#[tokio::test]
async fn discovers_caches_and_enforces_workload_bindings() {
    let (provider, task) = mock_provider().await;
    let state = OidcState::new(&[config(&provider.issuer)], "https://registry.example").unwrap();
    let token = sign(&payload(&provider.issuer), &provider.key);
    for _ in 0..2 {
        assert_eq!(state.workload(&token).await.unwrap().unwrap().identity.username, "ci");
    }
    assert_eq!(*provider.key_requests.lock().unwrap(), 1);
    let mut claims = payload(&provider.issuer);
    claims["repository_id"] = json!("456");
    assert!(state.workload(&sign(&claims, &provider.key)).await.is_err());
    claims["iss"] = json!("http://127.0.0.1:1");
    assert!(state.workload(&sign(&claims, &provider.key)).await.is_err());
    assert_eq!(*provider.key_requests.lock().unwrap(), 1);
    assert!(state.workload("ordinary-pnpr-token").await.unwrap().is_none());
    task.abort();
}

#[tokio::test]
async fn browser_login_uses_pkce_nonce_cookie_binding_and_single_use_state() {
    let (provider, task) = mock_provider().await;
    let mut config = config(&provider.issuer);
    config.login = Some(OidcLogin {
        client_secret: Some("secret".to_string()),
        users: vec![config.workloads.remove(0).identity],
    });
    let state = OidcState::new(&[config], "https://registry.example").unwrap();
    let start = state.start("example").await.unwrap();
    let url = Url::parse(&start.url).unwrap();
    let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["response_type"], "code");
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(query["redirect_uri"], "https://registry.example/-/oidc/example/callback");
    *provider.nonce.lock().unwrap() = Some(query["nonce"].clone());
    *provider.challenge.lock().unwrap() = Some(query["code_challenge"].clone());
    let session =
        state.finish("example", &start.state, &start.browser_secret, "code").await.unwrap();
    assert_eq!(state.session(&session.token).unwrap(), Some("ci".to_string()));
    assert!(session.expires <= Utc::now().timestamp() + 300);
    assert!(state.finish("example", &start.state, &start.browser_secret, "code").await.is_err());
    assert!(state.revoke_session(&session.token));
    assert!(state.session(&session.token).is_err());
    let start = state.start("example").await.unwrap();
    assert!(state.finish("example", &start.state, "other-browser", "code").await.is_err());
    let query: HashMap<_, _> = Url::parse(&start.url).unwrap().query_pairs().into_owned().collect();
    *provider.nonce.lock().unwrap() = Some(query["nonce"].clone());
    *provider.challenge.lock().unwrap() = Some(query["code_challenge"].clone());
    state.finish("example", &start.state, &start.browser_secret, "code").await.unwrap();
    task.abort();
}

#[test]
fn sessions_expire_and_configuration_fails_closed() {
    let state = OidcState::new(&[], "http://localhost").unwrap();
    assert!(state.issue_session("alice", 0).is_err());
    let session = state.issue_session("alice", Utc::now().timestamp() + 60).unwrap();
    state.sessions.lock().unwrap().values_mut().for_each(|session| session.expires = 0);
    assert!(state.session(&session.token).is_err());
    let mut config = config("https://issuer.example");
    config.name = "../bad".to_string();
    assert!(OidcState::new(&[config], "https://registry.example").is_err());
    assert!(token_payload(&"a".repeat(20_000)).is_err());
    assert!(validate_times(&json!({"iat": 1, "exp": 0})).is_err());
}

#[tokio::test]
async fn refresh_is_bounded_and_recovers_after_expiration() {
    let (provider, task) = mock_provider().await;
    let state = OidcState::new(&[config(&provider.issuer)], "https://registry.example").unwrap();
    let configured = state.providers.get("example").unwrap();
    state.metadata(configured, false).await.unwrap();
    for _ in 0..3 {
        state.metadata(configured, true).await.unwrap();
    }
    assert_eq!(*provider.key_requests.lock().unwrap(), 1);
    configured.metadata.lock().await.attempted_at =
        Some(Instant::now().checked_sub(Duration::from_secs(31)).unwrap());
    state.metadata(configured, true).await.unwrap();
    assert_eq!(*provider.key_requests.lock().unwrap(), 2);
    task.abort();
}

#[test]
fn verifies_rs256_workload_tokens() {
    use openidconnect::{
        PrivateSigningKey as _,
        core::{CoreJsonWebKeySet, CoreJwsSigningAlgorithm, CoreRsaPrivateSigningKey},
    };
    let key = CoreRsaPrivateSigningKey::from_pem(
        include_str!("../../tests/fixtures/oidc-test-key.pem"),
        None,
    )
    .unwrap();
    let issuer = "https://token.actions.githubusercontent.com";
    let metadata: CoreProviderMetadata = serde_json::from_value(metadata(issuer)).unwrap();
    let metadata = metadata.set_jwks(CoreJsonWebKeySet::new(vec![key.as_verification_key()]));
    let header = BASE64_URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256"}"#);
    let payload = BASE64_URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload(issuer)).unwrap());
    let message = format!("{header}.{payload}");
    let signature =
        key.sign(&CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256, message.as_bytes()).unwrap();
    let token = format!("{message}.{}", BASE64_URL_SAFE_NO_PAD.encode(signature));
    verify_workload(&config(issuer), &metadata, &token).unwrap();
}

#[tokio::test]
async fn rejects_expired_state_and_wrong_nonce() {
    let (provider, task) = mock_provider().await;
    let mut config = config(&provider.issuer);
    config.login =
        Some(OidcLogin { client_secret: None, users: vec![config.workloads.remove(0).identity] });
    let state = OidcState::new(&[config], "https://registry.example").unwrap();
    let start = state.start("example").await.unwrap();
    let query: HashMap<_, _> = Url::parse(&start.url).unwrap().query_pairs().into_owned().collect();
    *provider.nonce.lock().unwrap() = Some("wrong-nonce".to_string());
    *provider.challenge.lock().unwrap() = Some(query["code_challenge"].clone());
    assert!(state.finish("example", &start.state, &start.browser_secret, "code").await.is_err());
    assert!(state.sessions.lock().unwrap().is_empty());
    let start = state.start("example").await.unwrap();
    let mut login = state.open_login(&start.browser_secret).unwrap();
    login.expires = Utc::now().timestamp() - 1;
    let expired_cookie = state.seal_login(&login).unwrap();
    assert!(state.finish("example", &start.state, &expired_cookie, "code").await.is_err());
    task.abort();
}

#[tokio::test]
async fn selects_client_secret_post_from_discovery() {
    let (provider, task) = mock_provider().await;
    let mut config = config(&provider.issuer);
    config.login = Some(OidcLogin {
        client_secret: Some("secret".to_string()),
        users: vec![config.workloads.remove(0).identity],
    });
    let state = OidcState::new(&[config], "https://registry.example").unwrap();
    *provider.auth_method.lock().unwrap() = "client_secret_post".to_string();
    let start = state.start("example").await.unwrap();
    let query: HashMap<_, _> = Url::parse(&start.url).unwrap().query_pairs().into_owned().collect();
    *provider.nonce.lock().unwrap() = Some(query["nonce"].clone());
    *provider.challenge.lock().unwrap() = Some(query["code_challenge"].clone());
    state.finish("example", &start.state, &start.browser_secret, "code").await.unwrap();
    task.abort();
}

#[tokio::test]
async fn anonymous_login_starts_cannot_exhaust_or_evict_active_flows() {
    let (provider, task) = mock_provider().await;
    let mut config = config(&provider.issuer);
    config.login =
        Some(OidcLogin { client_secret: None, users: vec![config.workloads.remove(0).identity] });
    let state = OidcState::new(&[config], "https://registry.example").unwrap();
    let start = state.start("example").await.unwrap();
    let query: HashMap<_, _> = Url::parse(&start.url).unwrap().query_pairs().into_owned().collect();
    *provider.nonce.lock().unwrap() = Some(query["nonce"].clone());
    *provider.challenge.lock().unwrap() = Some(query["code_challenge"].clone());
    for _ in 0..super::MAX_ENTRIES + 32 {
        state.start("example").await.unwrap();
    }
    assert!(state.consumed.lock().unwrap().is_empty());
    state.finish("example", &start.state, &start.browser_secret, "code").await.unwrap();
    assert!(state.finish("example", &start.state, &start.browser_secret, "code").await.is_err());
    task.abort();
}

#[tokio::test]
async fn valid_workloads_do_not_wait_for_a_forced_network_refresh() {
    let (provider, task) = mock_provider().await;
    let state =
        Arc::new(OidcState::new(&[config(&provider.issuer)], "https://registry.example").unwrap());
    let token = sign(&payload(&provider.issuer), &provider.key);
    state.workload(&token).await.unwrap();
    state.providers["example"].metadata.lock().await.attempted_at =
        Instant::now().checked_sub(Duration::from_secs(31));
    provider.delay_discovery.store(true, std::sync::atomic::Ordering::Relaxed);
    let refreshing = Arc::clone(&state);
    let refresh =
        tokio::spawn(
            async move { refreshing.metadata(&refreshing.providers["example"], true).await },
        );
    tokio::time::timeout(Duration::from_secs(2), provider.discovery_started.notified())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_millis(100), state.workload(&token))
        .await
        .unwrap()
        .unwrap();
    provider.release_discovery.notify_one();
    refresh.await.unwrap().unwrap();
    task.abort();
}

#[tokio::test]
async fn failed_callbacks_are_rejected_on_repeated_and_concurrent_attempts() {
    let (provider, task) = mock_provider().await;
    let mut config = config(&provider.issuer);
    config.login =
        Some(OidcLogin { client_secret: None, users: vec![config.workloads.remove(0).identity] });
    let state = OidcState::new(&[config], "https://registry.example").unwrap();
    let start = state.start("example").await.unwrap();
    let first = state.finish("example", &start.state, &start.browser_secret, "invalid");
    let concurrent = state.finish("example", &start.state, &start.browser_secret, "invalid");
    let (first, concurrent) = tokio::join!(first, concurrent);
    assert!(first.is_err());
    assert!(concurrent.is_err());
    assert!(state.finish("example", &start.state, &start.browser_secret, "invalid").await.is_err());
    assert_eq!(*provider.token_requests.lock().unwrap(), 1);
    assert!(state.sessions.lock().unwrap().is_empty());
    task.abort();
}

#[tokio::test]
async fn callback_capacity_recovers_without_blocking_unattempted_logins() {
    let (provider, task) = mock_provider().await;
    let mut config = config(&provider.issuer);
    config.login =
        Some(OidcLogin { client_secret: None, users: vec![config.workloads.remove(0).identity] });
    let state = OidcState::new(&[config], "https://registry.example").unwrap();
    let start = state.start("example").await.unwrap();
    for index in 0..super::MAX_ENTRIES + 32 {
        state.record_attempt(&index.to_string()).unwrap();
    }
    assert_eq!(state.attempts.lock().unwrap().len(), super::MAX_ENTRIES);
    let permits = state.exchanges.try_acquire_many(16).unwrap();
    assert!(state.finish("example", &start.state, &start.browser_secret, "invalid").await.is_err());
    assert_eq!(*provider.token_requests.lock().unwrap(), 0);
    drop(permits);
    let query: HashMap<_, _> = Url::parse(&start.url).unwrap().query_pairs().into_owned().collect();
    *provider.nonce.lock().unwrap() = Some(query["nonce"].clone());
    *provider.challenge.lock().unwrap() = Some(query["code_challenge"].clone());
    state.finish("example", &start.state, &start.browser_secret, "code").await.unwrap();
    assert_eq!(*provider.token_requests.lock().unwrap(), 1);
    task.abort();
}
