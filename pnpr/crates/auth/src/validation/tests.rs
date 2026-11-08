use crate::UserBackend as _;

pub(crate) async fn assert_password_policy(users: &dyn crate::UserBackend) {
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
    for password in ["", "short", "ééééééé", &"a".repeat(72)] {
        let err = users.add_or_login("member", password).await.unwrap_err();
        assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
    }
}

pub(crate) async fn assert_username_skeletons(users: &dyn crate::UserBackend) {
    users.add_or_login("scope", "password").await.unwrap();
    let err = users.provision_user("scope").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::CONFLICT);
    assert!(matches!(err, crate::RegistryError::AdminConflict { .. }));
    for name in ["scоpe", "scοpe"] {
        let err = users.add_or_login(name, "password").await.unwrap_err();
        assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
        let err = users.create_user(name, "password").await.unwrap_err();
        assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
        let err = users.provision_user(name).await.unwrap_err();
        assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(err.to_string(), crate::validation::confusable_username().to_string());
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
    let err = users.rename_user(&id, "scоpe").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(err.to_string(), crate::validation::confusable_username().to_string());
    let err = users.rename_user(&id, "scope").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::CONFLICT);
    assert!(matches!(err, crate::RegistryError::AdminConflict { .. }));
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

#[tokio::test]
async fn imports_reject_names_confusable_with_saved_accounts() {
    assert_import_rejects_confusable_names("modern", &["rnodern"]).await;
}

#[tokio::test]
async fn imports_reject_confusable_names_in_the_same_batch() {
    assert_import_rejects_confusable_names("anchor", &["modern", "rnodern"]).await;
}

async fn assert_import_rejects_confusable_names(existing_name: &str, imported_names: &[&str]) {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("accounts.json");
    let users = crate::UserStore::open_with_cost(path.clone(), pnpr_config::MaxUsers::Unlimited, 4)
        .unwrap();
    users.add_or_login(existing_name, "password").await.unwrap();
    let saved = std::fs::read(&path).unwrap();
    let hash = bcrypt::hash("imported-password", 4).unwrap();
    let hashes: std::collections::HashMap<_, _> = imported_names
        .iter()
        .map(|name| ((*name).to_string(), hash.clone()))
        .collect();
    for mode in [crate::ImportMode::DryRun, crate::ImportMode::Persist] {
        for collisions in [crate::PasswordCollisions::Keep, crate::PasswordCollisions::Overwrite] {
            let options = crate::PasswordImport { collisions, mode };
            let error = crate::UserStore::import_password_hashes(&path, hashes.clone(), options)
                .unwrap_err();
            assert_eq!(error.status_code(), axum::http::StatusCode::BAD_REQUEST);
            assert_eq!(std::fs::read(&path).unwrap(), saved);
        }
    }
    let reopened =
        crate::UserStore::open_with_cost(path, pnpr_config::MaxUsers::Unlimited, 4).unwrap();
    assert_eq!(reopened.list_users().await.unwrap(), [existing_name]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn separate_file_stores_reject_concurrent_confusable_registrations() {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("accounts.json");
    // A slower hash lets both stores observe the missing name before either writes.
    let bcrypt_cost = 8;
    let first_store = crate::UserStore::open_with_cost(
        path.clone(),
        pnpr_config::MaxUsers::Unlimited,
        bcrypt_cost,
    )
    .unwrap();
    let second_store = crate::UserStore::open_with_cost(
        path.clone(),
        pnpr_config::MaxUsers::Unlimited,
        bcrypt_cost,
    )
    .unwrap();
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(3));
    let first_barrier = std::sync::Arc::clone(&barrier);
    let first = tokio::spawn(async move {
        first_barrier.wait().await;
        first_store.add_or_login("modern", "password").await
    });
    let second_barrier = std::sync::Arc::clone(&barrier);
    let second = tokio::spawn(async move {
        second_barrier.wait().await;
        second_store.add_or_login("rnodern", "password").await
    });
    barrier.wait().await;
    let results = (first.await.unwrap(), second.await.unwrap());
    let (winner, error) = match results {
        (Ok((crate::UpsertOutcome::Created, _)), Err(error)) => ("modern", error),
        (Err(error), Ok((crate::UpsertOutcome::Created, _))) => ("rnodern", error),
        results => panic!("expected one registration and one rejection, got {results:?}"),
    };
    assert_eq!(error.status_code(), axum::http::StatusCode::BAD_REQUEST);
    let reopened =
        crate::UserStore::open_with_cost(path, pnpr_config::MaxUsers::Unlimited, 4).unwrap();
    assert_eq!(reopened.list_users().await.unwrap(), [winner]);
    assert_eq!(
        reopened.add_or_login(winner, "password").await.unwrap().0,
        crate::UpsertOutcome::LoggedIn,
    );
}

#[tokio::test]
async fn file_password_policy_applies_to_registration_and_login() {
    let users = crate::UserStore::in_memory_with_cost(pnpr_config::MaxUsers::Unlimited, 4);
    assert_password_policy(&users).await;
    users.insert_user_for_test("imported".to_string(), bcrypt::hash("short", 4).unwrap());
    let error = users.add_or_login("imported", "short").await.unwrap_err();
    assert_eq!(error.status_code(), axum::http::StatusCode::BAD_REQUEST);
}
