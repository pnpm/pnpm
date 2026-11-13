use pnpr::{
    BackendConfig, Config, SqliteAuth, UserBackend,
    htpasswd_import::{HtpasswdImportOptions, import_htpasswd},
};
use pnpr_config::MaxUsers;
use pnpr_fixtures::MockConfigPresets as _;

/// A config whose accounts live in the SQLite database under
/// `storage`, and that database.
fn config_and_accounts(storage: &std::path::Path) -> (Config, SqliteAuth) {
    let config = Config::static_serve("127.0.0.1:0".parse().unwrap(), storage.to_path_buf());
    let BackendConfig::Sqlite(path) = &config.identity.backend else {
        panic!("a static-serve config keeps accounts in its SQLite database");
    };
    let users = SqliteAuth::open(path, MaxUsers::Unlimited).unwrap().with_bcrypt_cost(4);
    (config, users)
}

#[tokio::test]
async fn importing_a_reused_name_creates_a_fresh_identity() {
    for overwrite in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("htpasswd");
        let (config, users) = config_and_accounts(&directory.path().join("storage"));
        let (_, original_id) = users.add_or_login("member", "password").await.unwrap();
        let hash = users
            .credential("member")
            .await
            .unwrap()
            .unwrap()
            .into_password_hash()
            .unwrap();
        std::fs::write(&source, format!("member:{hash}\n")).unwrap();
        users.delete_user(&original_id).await.unwrap();
        let options = HtpasswdImportOptions { source, dry_run: false, overwrite };
        let report = import_htpasswd(&config, &options).await.unwrap();
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
    let (config, users) = config_and_accounts(&directory.path().join("storage"));
    let (_, id) = users.add_or_login("member", "old-password").await.unwrap();
    let hash = bcrypt::hash("new-password", 4).unwrap();
    std::fs::write(&source, format!("member:{hash}\n")).unwrap();
    let options = HtpasswdImportOptions { source, dry_run: false, overwrite: true };
    import_htpasswd(&config, &options).await.unwrap();
    assert_eq!(users.add_or_login("member", "new-password").await.unwrap().1, id);
    assert!(users.add_or_login("member", "old-password").await.is_err());
}

#[tokio::test]
async fn importing_into_a_shared_database_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("htpasswd");
    std::fs::write(&source, format!("member:{}\n", bcrypt::hash("password", 4).unwrap())).unwrap();
    let (mut config, _) = config_and_accounts(&directory.path().join("storage"));
    config.identity.backend = BackendConfig::Postgres(pnpr::SqlBackendSettings {
        url: "postgres://localhost/pnpr".to_string(),
        max_connections: None,
        timeout: pnpr::SqlBackendSettings::DEFAULT_TIMEOUT,
        startup_timeout: pnpr::SqlBackendSettings::DEFAULT_STARTUP_TIMEOUT,
    });
    let options = HtpasswdImportOptions { source, dry_run: false, overwrite: false };
    let error = import_htpasswd(&config, &options).await.unwrap_err();
    assert!(error.to_string().contains("SQLite database"), "{error}");
}
