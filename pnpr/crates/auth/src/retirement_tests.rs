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
        users
            .password_hash("renamed-member")
            .await
            .unwrap()
            .as_deref(),
        Some(crate::NO_PASSWORD_HASH),
    );
    assert!(users.add_or_login("renamed-member", "new-password").await.is_err());
    assert_eq!(users.account_id("renamed-member").await.unwrap(), Some(original.clone()));
    assert_eq!(users.delete_user(&original).await.unwrap(), UserRemoval::Removed);
    assert_eq!(users.delete_user(&original).await.unwrap(), UserRemoval::NoSuchUser);
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
    assert_eq!(records["member"]["id"], replacement);
}

#[tokio::test]
async fn a_renamed_account_keeps_its_tokens_and_a_reused_name_cannot_reactivate_them() {
    let auth = crate::AuthState::in_memory();
    let (_, id) = auth.users.add_or_login("member", "password").await.unwrap();
    let token = auth.tokens.issue(&id).await.unwrap();
    auth.users.rename_user(&id, "renamed").await.unwrap();
    assert_eq!(auth.tokens.lookup(&token).await.unwrap(), Some(id.clone()));
    auth.users.delete_user(&id).await.unwrap();
    let (_, replacement) = auth.users.add_or_login("renamed", "password").await.unwrap();
    assert_ne!(id, replacement);
    assert_eq!(auth.tokens.lookup(&token).await.unwrap(), None);
    assert_eq!(
        auth.tokens
            .find_by_key(&crate::sha256_hex(token.as_bytes()))
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
