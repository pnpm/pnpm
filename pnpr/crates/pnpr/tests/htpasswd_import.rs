use pnpr::{
    UserBackend, UserStore,
    htpasswd_import::{HtpasswdImportOptions, import_htpasswd},
};
use pnpr_config::MaxUsers;

#[tokio::test]
async fn importing_a_reused_name_creates_a_fresh_identity() {
    for overwrite in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("htpasswd");
        let target = directory.path().join("users.json");
        let users = UserStore::open_with_cost(target.clone(), MaxUsers::Unlimited, 4).unwrap();
        let (_, original_id) = users.add_or_login("member", "password").await.unwrap();
        let hash = users
            .password_hash("member")
            .await
            .unwrap()
            .unwrap();
        std::fs::write(&source, format!("member:{hash}\n")).unwrap();
        users.delete_user(&original_id).await.unwrap();
        let report = import_htpasswd(&HtpasswdImportOptions {
            source,
            target: Some(target),
            dry_run: false,
            overwrite,
        })
        .unwrap();
        assert_eq!(report.merged, 1);
        assert!(report.collisions.is_empty());
        let (_, imported_id) = users.add_or_login("member", "password").await.unwrap();
        assert_ne!(original_id, imported_id);
        assert_eq!(users.account_name(&original_id).await.unwrap(), None);
    }
}

#[tokio::test]
async fn overwriting_credentials_preserves_the_existing_account_id() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("htpasswd");
    let target = directory.path().join("users.json");
    let users = UserStore::open_with_cost(target.clone(), MaxUsers::Unlimited, 4).unwrap();
    let (_, id) = users.add_or_login("member", "old-password").await.unwrap();
    let hash = bcrypt::hash("new-password", 4).unwrap();
    std::fs::write(&source, format!("member:{hash}\n")).unwrap();
    import_htpasswd(&HtpasswdImportOptions {
        source,
        target: Some(target),
        dry_run: false,
        overwrite: true,
    })
    .unwrap();
    assert_eq!(users.add_or_login("member", "new-password").await.unwrap().1, id);
    assert!(users.add_or_login("member", "old-password").await.is_err());
}
