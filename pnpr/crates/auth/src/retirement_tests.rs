use crate::{PasswordChange, SqliteAuth, UserBackend, UserRemoval};
use pnpr_config::MaxUsers;

fn test_auth() -> SqliteAuth {
    SqliteAuth::in_memory(MaxUsers::Unlimited).unwrap().with_bcrypt_cost(4)
}

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
    assert_account_identity(&test_auth()).await;
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
    let users = test_auth();
    let id = users.provision_user("member").await.unwrap();
    assert_eq!(users.account_id("member").await.unwrap(), Some(id));
    assert!(users.add_or_login("member", "*").await.is_err());
    assert!(users.provision_user("member").await.is_err());
}
