use crate::{
    PasswordChange, RegistryError, UpsertOutcome, UserBackend, UserCreation, UserRemoval, UserStore,
};
use pnpr_config::MaxUsers;

#[tokio::test]
async fn retired_username_survives_restart_and_does_not_consume_registration_capacity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("htpasswd");
    let users = UserStore::open_with_cost(path.clone(), MaxUsers::Limited(1), 4).unwrap();
    users.add_or_login("former-member", "old-password").await.unwrap();
    assert_eq!(users.retire_user("former-member").await.unwrap(), UserRemoval::Removed);
    assert_eq!(users.retire_user("former-member").await.unwrap(), UserRemoval::NoSuchUser);
    assert!(!users.holds("former-member").unwrap());
    assert!(
        users
            .list_users()
            .await
            .unwrap()
            .is_empty(),
    );
    assert!(
        users
            .password_hash("former-member")
            .await
            .unwrap()
            .is_none(),
    );
    assert_eq!(users.delete_user("former-member").await.unwrap(), UserRemoval::NoSuchUser);
    drop(users);

    let users = UserStore::open_with_cost(path, MaxUsers::Limited(1), 4).unwrap();
    assert_eq!(users.retire_user("oidc-member").await.unwrap(), UserRemoval::NoSuchUser);
    assert!(users.is_retired("oidc-member").await.unwrap());
    assert!(
        users
            .password_hash("oidc-member")
            .await
            .unwrap()
            .is_none(),
    );

    assert!(users.is_retired("former-member").await.unwrap());
    for password in ["old-password", "new-password"] {
        assert!(matches!(
            users.add_or_login("former-member", password).await,
            Err(RegistryError::ReservedUsername { .. }),
        ));
    }
    assert_eq!(
        users.create_user("former-member", "new-password").await.unwrap(),
        UserCreation::NameTaken,
    );
    assert_eq!(
        users.set_password("former-member", "new-password").await.unwrap(),
        PasswordChange::NoSuchUser,
    );
    assert_eq!(
        users.add_or_login("new-member", "password").await.unwrap().0,
        UpsertOutcome::Created,
    );
}

#[tokio::test]
async fn capacity_tracks_external_file_changes_and_retirement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("htpasswd");
    let users = UserStore::open_with_cost(path.clone(), MaxUsers::Limited(1), 4).unwrap();
    users.add_or_login("first-member", "password").await.unwrap();
    assert!(matches!(
        users.add_or_login("second-member", "password").await,
        Err(RegistryError::TooManyUsers { .. }),
    ));
    let other = UserStore::open_with_cost(path.clone(), MaxUsers::Limited(1), 4).unwrap();
    other.retire_user("first-member").await.unwrap();
    users.add_or_login("second-member", "password").await.unwrap();
    assert_eq!(users.list_users().await.unwrap(), ["second-member"]);
    std::fs::remove_file(path).unwrap();
    users.add_or_login("third-member", "password").await.unwrap();
    assert_eq!(users.list_users().await.unwrap(), ["third-member"]);
}
