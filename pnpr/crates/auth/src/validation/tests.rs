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
    assert_rejected_unicode_usernames(users).await;
    assert_cltest_lookalikes_rejected(users).await;
    assert_deleted_accounts_release_skeletons(users).await;
}

async fn assert_rejected_unicode_usernames(users: &dyn crate::UserBackend) {
    for invalid_name in [
        "cl\u{0442}est",
        "sc\u{043e}pe",
        "sc\u{03bf}pe",
        "distin\u{0441}t",
        "cl\u{1d1b}est",
        "cl\u{200b}test",
        "cl\u{00ad}test",
        "\u{ff43}\u{ff4c}\u{ff54}\u{ff45}\u{ff53}\u{ff54}",
        "\u{202e}tsetlc",
        "\u{0301}cltest",
    ] {
        let err = users.add_or_login(invalid_name, "password").await.unwrap_err();
        assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST, "{invalid_name:?}");
        let err = users.create_user(invalid_name, "password").await.unwrap_err();
        assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST, "{invalid_name:?}");
        let err = users.provision_user(invalid_name).await.unwrap_err();
        assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST, "{invalid_name:?}");
    }
}

async fn assert_cltest_lookalikes_rejected(users: &dyn crate::UserBackend) {
    let (_, cltest_id) = users.add_or_login("cltest", "password").await.unwrap();
    let err = users.provision_user("cltest").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::CONFLICT);
    assert!(matches!(err, crate::RegistryError::AdminConflict { .. }));

    for lookalike in [
        "CLTEST",
        "Cltest",
        "cLtest",
        "clTest",
        "clt\u{00e9}st",
        "clte\u{0301}st",
        "cl\u{0167}est",
        "c1test",
    ] {
        let err = users.add_or_login(lookalike, "password").await.unwrap_err();
        assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST, "{lookalike:?}");
        assert_eq!(err.to_string(), crate::validation::confusable_username().to_string());
        let err = users.create_user(lookalike, "password").await.unwrap_err();
        assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST, "{lookalike:?}");
        assert_eq!(err.to_string(), crate::validation::confusable_username().to_string());
        let err = users.provision_user(lookalike).await.unwrap_err();
        assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST, "{lookalike:?}");
        assert_eq!(err.to_string(), crate::validation::confusable_username().to_string());
        assert!(
            users
                .account_id(lookalike)
                .await
                .unwrap()
                .is_none(),
        );
    }

    let other_id = users.provision_user("other").await.unwrap();
    for lookalike in ["CLTEST", "Cltest", "clt\u{00e9}st", "clte\u{0301}st"] {
        let err = users.rename_user(&other_id, lookalike).await.unwrap_err();
        assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST, "{lookalike:?}");
        assert_eq!(err.to_string(), crate::validation::confusable_username().to_string());
    }
    let err = users.rename_user(&other_id, "cl\u{0442}est").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
    let err = users.rename_user(&other_id, "cltest").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::CONFLICT);
    assert!(matches!(err, crate::RegistryError::AdminConflict { .. }));

    users.rename_user(&cltest_id, "Cltest").await.unwrap();
    assert_eq!(users.account_id("cltest").await.unwrap(), None);
    assert_eq!(
        users
            .account_id("Cltest")
            .await
            .unwrap()
            .as_deref(),
        Some(cltest_id.as_str()),
    );
    let err = users.add_or_login("cltest", "password").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);

    users.rename_user(&other_id, "distinct").await.unwrap();
    assert!(users.provision_user("DISTINCT").await.is_err());
    assert!(users.provision_user("distin\u{0441}t").await.is_err());

    users.create_user("modern", "password").await.unwrap();
    assert!(users.add_or_login("rnodern", "password").await.is_err());
    users.create_user("alice", "password").await.unwrap();
    assert!(users.add_or_login("ALICE", "password").await.is_err());
}

async fn assert_deleted_accounts_release_skeletons(users: &dyn crate::UserBackend) {
    let (_, scope_id) = users.add_or_login("scope", "password").await.unwrap();
    for lookalike in ["Scope", "SCOPE", "sc0pe", "sc\u{00f8}pe"] {
        let err = users.add_or_login(lookalike, "password").await.unwrap_err();
        assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST, "{lookalike:?}");
        assert_eq!(err.to_string(), crate::validation::confusable_username().to_string());
    }
    users.delete_user(&scope_id).await.unwrap();
    users.add_or_login("sc0pe", "password").await.unwrap();
    users.add_or_login("sc0pe", "password").await.unwrap();

    let (_, ace_id) = users.add_or_login("ace", "password").await.unwrap();
    let err = users.add_or_login("\u{0430}\u{0441}\u{0435}", "password").await.unwrap_err();
    assert_eq!(err.status_code(), axum::http::StatusCode::BAD_REQUEST);
    assert_eq!(err.to_string(), crate::validation::confusable_username().to_string());
    users.delete_user(&ace_id).await.unwrap();
    users.add_or_login("\u{0430}\u{0441}\u{0435}", "password").await.unwrap();
}

fn sqlite_auth(path: Option<&std::path::Path>, bcrypt_cost: u32) -> crate::SqliteAuth {
    let max_users = pnpr_config::MaxUsers::Unlimited;
    match path {
        Some(path) => crate::SqliteAuth::open(path, max_users),
        None => crate::SqliteAuth::in_memory(max_users),
    }
    .unwrap()
    .with_bcrypt_cost(bcrypt_cost)
}

#[tokio::test]
async fn sqlite_auth_rejects_confusable_names() {
    assert_username_skeletons(&sqlite_auth(None, 4)).await;
}

#[tokio::test]
async fn concurrent_confusable_registrations_create_only_one_account() {
    let users = sqlite_auth(None, 4);
    let modern = users.add_or_login("modern", "password");
    let rnodern = users.add_or_login("rnodern", "password");
    let (first, second) = tokio::join!(modern, rnodern);
    assert_ne!(first.is_ok(), second.is_ok());
    assert_eq!(users.list_users().await.unwrap().len(), 1);
}

#[tokio::test]
async fn imports_reject_names_confusable_with_saved_accounts() {
    assert_import_rejects_confusable_names("modern", &["rnodern"]).await;
    assert_import_rejects_confusable_names("cltest", &["CLTEST"]).await;
    assert_import_rejects_confusable_names("cltest", &["clt\u{00e9}st"]).await;
}

#[tokio::test]
async fn imports_reject_confusable_names_in_the_same_batch() {
    assert_import_rejects_confusable_names("anchor", &["modern", "rnodern"]).await;
    assert_import_rejects_confusable_names("anchor", &["cltest", "Cltest"]).await;
    assert_import_rejects_confusable_names("anchor", &["cltest", "clte\u{0301}st"]).await;
}

async fn assert_import_rejects_confusable_names(existing_name: &str, imported_names: &[&str]) {
    let users = sqlite_auth(None, 4);
    users.add_or_login(existing_name, "password").await.unwrap();
    let hash = bcrypt::hash("imported-password", 4).unwrap();
    let hashes: std::collections::HashMap<_, _> = imported_names
        .iter()
        .map(|name| ((*name).to_string(), hash.clone()))
        .collect();
    for dry_run in [true, false] {
        for collisions in [crate::PasswordCollisions::Keep, crate::PasswordCollisions::Overwrite] {
            let options = crate::PasswordImport { collisions, dry_run };
            let error = users.import_password_hashes(hashes.clone(), options).await.unwrap_err();
            assert_eq!(error.status_code(), axum::http::StatusCode::BAD_REQUEST);
            assert_eq!(users.list_users().await.unwrap(), [existing_name]);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn separate_connections_reject_concurrent_confusable_registrations() {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("metadata.sqlite");
    // A slower hash lets both stores observe the missing name before either writes.
    let first_store = sqlite_auth(Some(&path), 8);
    let second_store = sqlite_auth(Some(&path), 8);
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
    let reopened = sqlite_auth(Some(&path), 4);
    assert_eq!(reopened.list_users().await.unwrap(), [winner]);
    assert_eq!(
        reopened.add_or_login(winner, "password").await.unwrap().0,
        crate::UpsertOutcome::LoggedIn,
    );
}

#[tokio::test]
async fn sqlite_password_policy_applies_to_registration_and_login() {
    let users = sqlite_auth(None, 4);
    assert_password_policy(&users).await;
    let hashes = std::collections::HashMap::from([(
        "imported".to_string(),
        bcrypt::hash("short", 4).unwrap(),
    )]);
    let options =
        crate::PasswordImport { collisions: crate::PasswordCollisions::Keep, dry_run: false };
    users.import_password_hashes(hashes, options).await.unwrap();
    let error = users.add_or_login("imported", "short").await.unwrap_err();
    assert_eq!(error.status_code(), axum::http::StatusCode::BAD_REQUEST);
}

#[test]
fn skeleton_hash_equates_case_diacritics_and_homoglyphs() {
    use crate::validation::username_skeleton_hash;

    for (canonical, variants) in [
        (
            "cltest",
            &[
                "CLTEST",
                "Cltest",
                "cLtest",
                "clTest",
                "clt\u{00e9}st",
                "clte\u{0301}st",
                "cl\u{0167}est",
                "c1test",
            ][..],
        ),
        ("alice", &["ALICE", "al\u{0131}ce", "al\u{0130}ce"]),
        ("scope", &["Scope", "SCOPE", "sc0pe", "SC0PE", "sc\u{00f8}pe"]),
        ("stra\u{00df}e", &["STRASSE", "STRA\u{1e9e}E"]),
        ("\u{03c3}\u{03bf}\u{03c6}\u{03cc}\u{03c2}", &["\u{03a3}\u{039f}\u{03a6}\u{039f}\u{03a3}"]),
    ] {
        let expected = username_skeleton_hash(canonical);
        for variant in variants {
            assert_eq!(username_skeleton_hash(variant), expected, "{canonical:?} vs {variant:?}");
        }
    }

    assert_ne!(username_skeleton_hash("alice"), username_skeleton_hash("allce"));
    assert_ne!(
        username_skeleton_hash("\u{d55c}\u{ae00}"),
        username_skeleton_hash("\u{d63c}\u{ae00}"),
    );
}
