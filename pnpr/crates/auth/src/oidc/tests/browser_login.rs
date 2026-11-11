use super::*;

#[tokio::test]
async fn browser_login_binds_verified_subjects_through_scim() {
    let (provider, task) = mock_provider().await;
    let mut config = config(&provider.issuer);
    let subject = config.workloads.remove(0).identity.subject;
    config.login = Some(OidcLogin {
        scim: true,
        client_secret: Some("secret".to_string()),
        users: vec![],
        groups: Some(groups(&[("platform-eng", "platform")])),
    });
    let state = OidcState::new(&[config], "https://registry.example").unwrap();
    for resolves in [true, false] {
        let start = state.start("example", LoginReturn::Token).await.unwrap();
        let query: HashMap<_, _> = Url::parse(&start.url)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect();
        *provider.nonce.lock().unwrap() = Some(query["nonce"].clone());
        *provider.challenge.lock().unwrap() = Some(query["code_challenge"].clone());
        let session =
            state.finish("example", &start.state, &start.browser_secret, "code", |verified| {
                assert_eq!(verified, subject);
                if resolves { Ok("scim-account".to_string()) } else { Err(rejected()) }
            })
            .await;
        if resolves {
            let user = state
                .session(&session.unwrap().token)
                .unwrap()
                .unwrap();
            assert_eq!(user.user_id, "scim-account");
            assert_eq!(user.teams[0].team_id, "platform");
        } else {
            assert!(session.is_err());
        }
    }
    let start = state.start("example", LoginReturn::Token).await.unwrap();
    let query: HashMap<_, _> = Url::parse(&start.url)
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect();
    *provider.nonce.lock().unwrap() = Some("wrong-nonce".to_string());
    *provider.challenge.lock().unwrap() = Some(query["code_challenge"].clone());
    assert!(
        state
            .finish("example", &start.state, &start.browser_secret, "code", |_| {
                panic!("unverified claims must not reach SCIM binding")
            })
            .await
            .is_err()
    );
    task.abort();
}

#[test]
fn scim_and_static_login_bindings_are_mutually_exclusive() {
    let mut config = config("https://identity.example");
    config.login = Some(OidcLogin {
        scim: true,
        client_secret: None,
        users: vec![config.workloads.remove(0).identity],
        groups: None,
    });
    assert!(OidcState::new(&[config], "https://registry.example").is_err());
}

#[tokio::test]
async fn browser_login_uses_pkce_nonce_cookie_binding_and_single_use_state() {
    let (provider, task) = mock_provider().await;
    let mut config = config(&provider.issuer);
    config.login = Some(OidcLogin {
        scim: false,
        client_secret: Some("secret".to_string()),
        users: vec![config.workloads.remove(0).identity],
        groups: Some(groups(&[("platform-eng", "platform"), ("finance", "finance")])),
    });
    let state = OidcState::new(&[config], "https://registry.example").unwrap();
    let start = state.start("example", LoginReturn::Token).await.unwrap();
    let url = Url::parse(&start.url).unwrap();
    let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["response_type"], "code");
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(query["redirect_uri"], "https://registry.example/-/oidc/example/callback");
    *provider.nonce.lock().unwrap() = Some(query["nonce"].clone());
    *provider.challenge.lock().unwrap() = Some(query["code_challenge"].clone());
    let session = state
        .finish("example", &start.state, &start.browser_secret, "code", |_| Err(rejected()))
        .await
        .unwrap();
    let user = state
        .session(&session.token)
        .unwrap()
        .unwrap();
    assert_eq!(user.user_id, "ci");
    let teams: Vec<&str> = user.teams
        .iter()
        .map(|grant| grant.team_id.as_str())
        .collect();
    assert_eq!(teams, ["platform"]);
    assert!(session.expires <= Utc::now().timestamp() + 300);
    assert_eq!(session.returns_to, LoginReturn::Token);
    assert!(
        state
            .finish("example", &start.state, &start.browser_secret, "code", |_| Err(rejected()))
            .await
            .is_err()
    );
    assert!(state.revoke_session(&session.token));
    assert!(state.session(&session.token).is_err());
    let start = state.start("example", LoginReturn::Ui).await.unwrap();
    assert!(
        state
            .finish("example", &start.state, "other-browser", "code", |_| Err(rejected()))
            .await
            .is_err()
    );
    let query: HashMap<_, _> = Url::parse(&start.url)
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect();
    *provider.nonce.lock().unwrap() = Some(query["nonce"].clone());
    *provider.challenge.lock().unwrap() = Some(query["code_challenge"].clone());
    let session = state
        .finish("example", &start.state, &start.browser_secret, "code", |_| Err(rejected()))
        .await
        .unwrap();
    assert_eq!(session.returns_to, LoginReturn::Ui);
    task.abort();
}
