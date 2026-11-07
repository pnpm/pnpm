use crate::UserBackend as _;

#[tokio::test]
async fn password_policy_applies_to_new_credentials_and_preserves_legacy_login() {
    let users = crate::UserStore::in_memory_with_cost(pnpr_config::MaxUsers::Unlimited, 4);
    for password in ["", "short", "ééééééé", &"a".repeat(72)] {
        let err = users.add_or_login("member", password).await.unwrap_err();
        assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
        assert!(
            users
                .account_id("member")
                .await
                .unwrap()
                .is_none(),
        );
        assert!(users.create_user("member", password).await.is_err());
    }
    users.add_or_login("member", "éééééééé").await.unwrap();
    let id = users
        .account_id("member")
        .await
        .unwrap()
        .unwrap();
    assert!(users.set_password(&id, "short").await.is_err());
    users.add_or_login("member", "éééééééé").await.unwrap();
    users.insert_user_for_test("legacy".to_string(), bcrypt::hash("short", 4).unwrap());
    assert_eq!(
        users.add_or_login("legacy", "short").await.unwrap().0,
        crate::UpsertOutcome::LoggedIn,
    );
}

pub(crate) async fn assert_username_skeletons(users: &dyn crate::UserBackend) {
    users.add_or_login("scope", "password").await.unwrap();
    for name in ["scоpe", "scοpe"] {
        let err = users.add_or_login(name, "password").await.unwrap_err();
        assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
        let err = users.create_user(name, "password").await.unwrap_err();
        assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
        assert!(users.provision_user(name).await.is_err());
        assert!(
            users
                .account_id(name)
                .await
                .unwrap()
                .is_none(),
        );
    }
    users.create_user("modern", "password").await.unwrap();
    assert!(users.add_or_login("rnodern", "password").await.is_err());
    let id = users.provision_user("other").await.unwrap();
    assert!(users.rename_user(&id, "scоpe").await.is_err());
    users.rename_user(&id, "distinct").await.unwrap();
    assert!(users.provision_user("distinсt").await.is_err());
    let scope_id = users
        .account_id("scope")
        .await
        .unwrap()
        .unwrap();
    users.delete_user(&scope_id).await.unwrap();
    users.add_or_login("scоpe", "password").await.unwrap();
    users.add_or_login("scоpe", "password").await.unwrap();
}

#[tokio::test]
async fn user_store_rejects_confusable_names() {
    let users = crate::UserStore::in_memory_with_cost(pnpr_config::MaxUsers::Unlimited, 4);
    assert_username_skeletons(&users).await;
}

#[tokio::test]
async fn concurrent_confusable_registrations_create_only_one_account() {
    let users = crate::UserStore::in_memory_with_cost(pnpr_config::MaxUsers::Unlimited, 4);
    let modern = users.add_or_login("modern", "password");
    let rnodern = users.add_or_login("rnodern", "password");
    let (first, second) = tokio::join!(modern, rnodern);
    assert_ne!(first.is_ok(), second.is_ok());
    assert_eq!(users.list_users().await.unwrap().len(), 1);
}
