use super::{Config, TempDir, check_config};

#[tokio::test]
async fn config_check_rejects_insecure_database_tokens() {
    let dir = TempDir::new().unwrap();
    let config_file = dir.path().join("pnpr.yaml");
    std::fs::write(
        &config_file,
        "database:\n  url: http://database.example.test:8080\n  authToken: example-secret-token\n",
    )
    .unwrap();
    let check = check_config(Some(&config_file)).unwrap_err().to_string();
    assert!(!check.contains("example-secret-token"), "{check}");
    #[cfg(feature = "backend-libsql")]
    assert!(check.contains("database.authToken"), "{check}");
    #[cfg(not(feature = "backend-libsql"))]
    assert!(check.contains("database driver"), "{check}");
    let config = Config::from_yaml(&config_file, "127.0.0.1:0".parse().unwrap(), None).unwrap();
    let error = pnpr::serve(config).await.unwrap_err().to_string();
    assert_eq!(error, check);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[cfg(not(feature = "backend-postgres"))]
#[tokio::test]
async fn config_check_rejects_a_database_driver_missing_from_the_build() {
    let dir = TempDir::new().unwrap();
    let config_file = dir.path().join("pnpr.yaml");
    std::fs::write(
        &config_file,
        "database:\n  url: postgres://example:secret@database.example.test/registry\n",
    )
    .unwrap();
    let error = check_config(Some(&config_file)).unwrap_err().to_string();
    assert!(error.contains("database driver"), "{error}");
    assert!(!error.contains("secret"), "{error}");
    let config = Config::from_yaml(&config_file, "127.0.0.1:0".parse().unwrap(), None).unwrap();
    assert_eq!(pnpr::serve(config).await.unwrap_err().to_string(), error);
}
