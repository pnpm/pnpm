use crate::{PasswordChange, UserBackend, UserRemoval, UserStore};
use pnpr_config::MaxUsers;

pub(crate) async fn assert_account_identity(users: &dyn UserBackend) {
    let (_, original) = users.add_or_login("former-member", "old-password").await.unwrap();
    assert_eq!(users.account_id("former-member").await.unwrap(), Some(original.clone()));
    users.rename_user(&original, "renamed-member").await.unwrap();
    users.rename_user(&original, "renamed-member").await.unwrap();
    assert_eq!(users.account_id("former-member").await.unwrap(), None);
    assert_eq!(users.account_id("renamed-member").await.unwrap(), Some(original.clone()));
    assert_eq!(
        users
            .accounts_by_id(&[original.clone(), "missing".to_string()])
            .await
            .unwrap(),
        vec![crate::UserAccount { id: original.clone(), username: "renamed-member".to_string() }],
    );
    assert!(
        users
            .accounts_by_id(&[])
            .await
            .unwrap()
            .is_empty(),
    );
    assert_eq!(users.add_or_login("renamed-member", "old-password").await.unwrap().1, original);
    assert_eq!(
        users.set_password(&original, "new-password").await.unwrap(),
        PasswordChange::Changed,
    );
    assert_eq!(users.add_or_login("renamed-member", "new-password").await.unwrap().1, original);
    assert_eq!(users.clear_password(&original).await.unwrap(), PasswordChange::Changed);
    assert_eq!(
        users.credential("renamed-member").await.unwrap(),
        Some(crate::AccountCredential::None),
    );
    assert!(users.add_or_login("renamed-member", "new-password").await.is_err());
    assert_eq!(users.account_status(&original).await.unwrap(), Some(crate::AccountStatus::Active));
    assert_eq!(
        users
            .status_revisions(&[original.clone(), "missing".to_string()])
            .await
            .unwrap(),
        std::collections::HashMap::from([(
            original.clone(),
            crate::AccountStatusRevision::reconciled(crate::AccountStatus::Active, 0),
        )]),
    );
    assert!(
        users
            .status_revisions(&[])
            .await
            .unwrap()
            .is_empty(),
    );
    assert_eq!(
        users
            .apply_account_status(&original, 1, crate::AccountLifecycle::Deactivated)
            .await
            .unwrap(),
        crate::AccountStatusOutcome::Applied,
    );
    assert_eq!(
        users.account_status(&original).await.unwrap(),
        Some(crate::AccountStatus::Deactivated),
    );
    assert_eq!(
        users
            .status_revisions(std::slice::from_ref(&original))
            .await
            .unwrap()
            .get(&original)
            .copied(),
        Some(crate::AccountStatusRevision::reconciled(crate::AccountStatus::Deactivated, 1)),
    );
    assert_eq!(users.account_id("renamed-member").await.unwrap(), Some(original.clone()));
    assert_eq!(users.active_account_id("renamed-member").await.unwrap(), None);
    assert_eq!(users.active_account_name(&original).await.unwrap(), None);
    assert_eq!(users.credential("renamed-member").await.unwrap(), None);
    assert_eq!(users.list_users().await.unwrap(), ["renamed-member"]);
    assert_eq!(
        users
            .list_accounts()
            .await
            .unwrap()
            .len(),
        1,
    );
    assert_eq!(
        users
            .add_or_login("renamed-member", "new-password")
            .await
            .unwrap_err()
            .status_code(),
        403,
    );
    assert_eq!(
        users.apply_account_status(&original, 2, crate::AccountLifecycle::Active).await.unwrap(),
        crate::AccountStatusOutcome::Applied,
    );
    assert_eq!(
        users
            .status_revisions(std::slice::from_ref(&original))
            .await
            .unwrap()
            .get(&original)
            .copied(),
        Some(crate::AccountStatusRevision::reconciled(crate::AccountStatus::Active, 2)),
    );
    assert_eq!(users.active_account_id("renamed-member").await.unwrap(), Some(original.clone()));
    assert_eq!(
        users
            .active_account_name(&original)
            .await
            .unwrap()
            .as_deref(),
        Some("renamed-member"),
    );
    assert_eq!(users.delete_user(&original).await.unwrap(), UserRemoval::Removed);
    assert_eq!(users.delete_user(&original).await.unwrap(), UserRemoval::NoSuchUser);
    assert_eq!(users.account_status(&original).await.unwrap(), Some(crate::AccountStatus::Deleted));
    assert_eq!(
        users
            .status_revisions(std::slice::from_ref(&original))
            .await
            .unwrap()
            .get(&original)
            .copied(),
        Some(crate::AccountStatusRevision::reconciled(crate::AccountStatus::Deleted, 2)),
    );
    assert_eq!(
        users.apply_account_status(&original, 3, crate::AccountLifecycle::Active).await.unwrap(),
        crate::AccountStatusOutcome::Missing,
    );
    assert_eq!(users.account_name(&original).await.unwrap(), None);
    assert_eq!(users.clear_password(&original).await.unwrap(), PasswordChange::NoSuchUser);
    assert_eq!(
        users
            .rename_user(&original, "resurrected")
            .await
            .unwrap_err()
            .status_code(),
        404,
    );
    let (_, replacement) = users.add_or_login("renamed-member", "new-password").await.unwrap();
    assert_ne!(original, replacement);
    assert_eq!(
        users
            .accounts_by_id(&[original.clone(), replacement.clone()])
            .await
            .unwrap(),
        vec![crate::UserAccount {
            id: replacement.clone(),
            username: "renamed-member".to_string()
        }],
    );
    assert_eq!(
        users.set_password(&original, "stale-write").await.unwrap(),
        PasswordChange::NoSuchUser,
    );
    assert_eq!(
        users
            .account_name(&replacement)
            .await
            .unwrap()
            .as_deref(),
        Some("renamed-member"),
    );
}

#[tokio::test]
async fn account_ids_survive_rename_and_password_changes_but_are_never_reassigned() {
    assert_account_identity(&UserStore::in_memory_with_cost(MaxUsers::Unlimited, 4)).await;
}

#[tokio::test]
async fn account_ids_and_deleted_records_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("users.json");
    let users = UserStore::open_with_cost(path.clone(), MaxUsers::Limited(1), 4).unwrap();
    let (_, id) = users.add_or_login("member", "password").await.unwrap();
    let reopened = UserStore::open_with_cost(path.clone(), MaxUsers::Limited(1), 4).unwrap();
    assert_eq!(reopened.account_id("member").await.unwrap(), Some(id.clone()));
    reopened.delete_user(&id).await.unwrap();
    let (_, replacement) = users.add_or_login("member", "password").await.unwrap();
    assert_ne!(id, replacement);
    let records: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(records[format!("deleted:{id}")]["id"], id);
    assert_eq!(records[format!("deleted:{id}")]["status"], "deleted");
    assert_eq!(records[format!("deleted:{id}")]["credential_kind"], "none");
    assert!(records[format!("deleted:{id}")].get("password_hash").is_none());
    assert!(records[format!("deleted:{id}")]["created_at"].is_u64());
    assert_eq!(records["member"]["id"], replacement);
    assert_eq!(records["member"]["status"], "active");
    assert_eq!(records["member"]["credential_kind"], "bcrypt");
    assert!(records["member"]["created_at"].is_u64());
}

#[tokio::test]
async fn a_renamed_account_keeps_its_tokens_and_a_reused_name_cannot_reactivate_them() {
    let auth = crate::AuthState::in_memory();
    let (_, id) = auth.users.add_or_login("member", "password").await.unwrap();
    let token = auth.tokens.issue(&id).await.unwrap();
    auth.users.rename_user(&id, "renamed").await.unwrap();
    assert_eq!(auth.tokens.lookup(&token).await.unwrap(), Some(id.clone()));
    assert_eq!(
        auth.users
            .apply_account_status(&id, 1, crate::AccountLifecycle::Deactivated)
            .await
            .unwrap(),
        crate::AccountStatusOutcome::Applied,
    );
    assert_eq!(auth.tokens.lookup(&token).await.unwrap(), None);
    assert!(
        auth.tokens
            .find_by_key(&crate::sha256_hex(token.as_bytes()))
            .await
            .unwrap()
            .is_none(),
    );
    let dormant = auth.tokens.issue(&id).await.unwrap();
    assert_eq!(auth.tokens.lookup(&dormant).await.unwrap(), None);
    assert!(
        auth.tokens
            .lookup_record(&dormant)
            .await
            .unwrap()
            .is_none(),
    );
    assert_eq!(
        auth.users.apply_account_status(&id, 2, crate::AccountLifecycle::Active).await.unwrap(),
        crate::AccountStatusOutcome::Applied,
    );
    assert_eq!(auth.tokens.lookup(&token).await.unwrap(), None);
    assert_eq!(auth.tokens.lookup(&dormant).await.unwrap(), Some(id.clone()));
    assert_eq!(
        auth.tokens
            .lookup_record(&dormant)
            .await
            .unwrap()
            .unwrap()
            .owner_name(),
        Some("renamed"),
    );
    auth.users.delete_user(&id).await.unwrap();
    let (_, replacement) = auth.users.add_or_login("renamed", "password").await.unwrap();
    assert_ne!(id, replacement);
    assert_eq!(auth.tokens.lookup(&dormant).await.unwrap(), None);
    assert_eq!(
        auth.tokens
            .find_by_key(&crate::sha256_hex(dormant.as_bytes()))
            .await
            .unwrap()
            .unwrap()
            .user_id,
        id,
    );
}

#[tokio::test]
async fn passwordless_accounts_have_ids_and_cannot_be_self_registered() {
    let users = UserStore::in_memory_with_cost(MaxUsers::Unlimited, 4);
    let id = users.provision_user("member").await.unwrap();
    assert_eq!(users.account_id("member").await.unwrap(), Some(id));
    assert!(users.add_or_login("member", "*").await.is_err());
    assert!(users.provision_user("member").await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_sqlite_token_issuance_and_revocation_keep_memory_and_sqlite_aligned() {
    use crate::TokenBackend as _;
    let dir = tempfile::tempdir().unwrap();
    let tokens_path = dir.path().join("tokens.db");
    let raw_store = std::sync::Arc::new(crate::TokenStore::open(tokens_path.clone()).unwrap());
    let users = std::sync::Arc::new(UserStore::in_memory_with_cost(MaxUsers::Unlimited, 4));
    let (_, id) = users.add_or_login("member", "password").await.unwrap();
    let initial = raw_store.issue(&id).await.unwrap();
    assert_eq!(raw_store.lookup(&initial).await.unwrap(), Some(id.clone()));
    {
        let persist_guard = raw_store.lock_persist_for_test();
        let mut revoke_fut = raw_store.revoke_all_for_user(&id);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        let first_poll = revoke_fut.as_mut().poll(&mut cx);
        assert!(first_poll.is_pending(), "revoke_all_for_user must enter blocking worker");
        drop(revoke_fut);
        drop(persist_guard);
    }
    let finish_path = tokens_path.clone();
    let finish_store = std::sync::Arc::clone(&raw_store);
    tokio::task::spawn_blocking(move || drop(finish_store.lock_persist_for_test()))
        .await
        .unwrap();
    assert!(
        raw_store
            .list_for_user(&id)
            .await
            .unwrap()
            .is_empty(),
    );
    assert!(
        crate::TokenStore::open(finish_path)
            .unwrap()
            .list_for_user(&id)
            .await
            .unwrap()
            .is_empty(),
    );

    drop(raw_store);
    let tokens = std::sync::Arc::new(crate::OwnedTokens::new(
        crate::TokenStore::open(tokens_path.clone()).unwrap(),
        std::sync::Arc::clone(&users),
    ));
    for round in 1..=20_u64 {
        let _ = users.apply_account_status(&id, round * 2, crate::AccountLifecycle::Active).await;
        let issue_tokens = std::sync::Arc::clone(&tokens);
        let issue_id = id.clone();
        let issue_task = tokio::spawn(async move { issue_tokens.issue(&issue_id).await.unwrap() });
        users
            .apply_account_status(&id, round * 2 + 1, crate::AccountLifecycle::Deactivated)
            .await
            .unwrap();
        let _ = issue_task.await.unwrap();
        let mut live: Vec<String> = tokens
            .list_for_user(&id)
            .await
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        let reopened = crate::TokenStore::open(tokens_path.clone()).unwrap();
        let mut disk: Vec<String> = reopened
            .list_for_user(&id)
            .await
            .unwrap()
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        live.sort();
        disk.sort();
        assert_eq!(live, disk);
    }
}

#[tokio::test]
async fn failed_token_revocation_keeps_deactivation_pending_and_blocks_activation() {
    use crate::TokenBackend as _;
    let dir = tempfile::tempdir().unwrap();
    let users_path = dir.path().join("users.json");
    let tokens_path = dir.path().join("tokens.db");
    let backup_path = dir.path().join("tokens.db.bak");
    let users = std::sync::Arc::new(
        UserStore::open_with_cost(users_path.clone(), MaxUsers::Unlimited, 4).unwrap(),
    );
    let tokens = crate::OwnedTokens::new(
        crate::TokenStore::open(tokens_path.clone()).unwrap(),
        std::sync::Arc::clone(&users),
    );
    let auth = crate::AuthState {
        users: std::sync::Arc::clone(&users) as std::sync::Arc<dyn UserBackend>,
        tokens: std::sync::Arc::new(tokens),
    };
    let (_, id) = auth.users.add_or_login("member", "password").await.unwrap();
    let token = auth.tokens.issue(&id).await.unwrap();

    std::fs::rename(&tokens_path, &backup_path).unwrap();
    std::fs::create_dir(&tokens_path).unwrap();

    assert!(auth.apply_account_status(&id, 1, crate::AccountLifecycle::Deactivated).await.is_err());
    assert_eq!(
        auth.users
            .status_revisions(std::slice::from_ref(&id))
            .await
            .unwrap()
            .get(&id)
            .copied(),
        Some(crate::AccountStatusRevision {
            status: crate::AccountStatus::Deactivated,
            revision: 1,
            pending_token_revocation: true,
        }),
    );
    assert_eq!(auth.tokens.lookup(&token).await.unwrap(), None);
    assert!(auth.apply_account_status(&id, 2, crate::AccountLifecycle::Active).await.is_err());
    assert_eq!(
        auth.users.account_status(&id).await.unwrap(),
        Some(crate::AccountStatus::Deactivated),
    );

    let reopened_users =
        UserStore::open_with_cost(users_path.clone(), MaxUsers::Unlimited, 4).unwrap();
    assert_eq!(
        reopened_users
            .status_revisions(std::slice::from_ref(&id))
            .await
            .unwrap()
            .get(&id)
            .copied(),
        Some(crate::AccountStatusRevision {
            status: crate::AccountStatus::Deactivated,
            revision: 1,
            pending_token_revocation: true,
        }),
    );
    assert!(
        reopened_users.apply_account_status(&id, 2, crate::AccountLifecycle::Active).await.is_err(),
    );

    std::fs::remove_dir(&tokens_path).unwrap();
    std::fs::rename(&backup_path, &tokens_path).unwrap();

    assert_eq!(
        auth.apply_account_status(&id, 1, crate::AccountLifecycle::Deactivated)
            .await
            .unwrap(),
        crate::AccountStatusOutcome::Applied,
    );
    assert_eq!(
        auth.users
            .status_revisions(std::slice::from_ref(&id))
            .await
            .unwrap()
            .get(&id)
            .copied(),
        Some(crate::AccountStatusRevision::reconciled(crate::AccountStatus::Deactivated, 1)),
    );
    assert_eq!(
        auth.apply_account_status(&id, 2, crate::AccountLifecycle::Active).await.unwrap(),
        crate::AccountStatusOutcome::Applied,
    );
    assert_eq!(auth.tokens.lookup(&token).await.unwrap(), None);
    assert!(
        crate::TokenStore::open(tokens_path)
            .unwrap()
            .list_for_user(&id)
            .await
            .unwrap()
            .is_empty(),
    );
}

#[tokio::test]
async fn deactivation_revokes_tokens_across_all_bound_owned_token_stores() {
    use crate::TokenBackend as _;
    let users = std::sync::Arc::new(UserStore::in_memory_with_cost(MaxUsers::Unlimited, 4));
    let (_, id) = users.add_or_login("member", "password").await.unwrap();
    let first =
        crate::OwnedTokens::new(crate::TokenStore::in_memory(), std::sync::Arc::clone(&users));
    let second =
        crate::OwnedTokens::new(crate::TokenStore::in_memory(), std::sync::Arc::clone(&users));
    let first_token = first.issue(&id).await.unwrap();
    let second_token = second.issue(&id).await.unwrap();
    users.apply_account_status(&id, 1, crate::AccountLifecycle::Deactivated).await.unwrap();
    users.apply_account_status(&id, 2, crate::AccountLifecycle::Active).await.unwrap();
    assert_eq!(first.lookup(&first_token).await.unwrap(), None);
    assert_eq!(second.lookup(&second_token).await.unwrap(), None);
}
