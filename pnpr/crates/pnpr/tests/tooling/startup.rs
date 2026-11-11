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
    let router_error = pnpr::try_router_with_auth(config.clone(), pnpr::AuthState::in_memory())
        .err()
        .unwrap()
        .to_string();
    assert_eq!(router_error, check);
    let error = pnpr::serve(config).await.unwrap_err().to_string();
    assert_eq!(error, check);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn config_check_rejects_invalid_database_urls_without_echoing_credentials() {
    for url in [
        "libsql://",
        "postgres:example:secret@database.example.test/registry",
        "postgresql:example:secret@database.example.test/registry",
        "mysql:example:secret@database.example.test/registry",
    ] {
        let dir = TempDir::new().unwrap();
        let config_file = dir.path().join("pnpr.yaml");
        std::fs::write(&config_file, format!("database:\n  url: {url}\n")).unwrap();
        let error = check_config(Some(&config_file)).unwrap_err().to_string();
        assert!(!error.contains("secret"), "{url}: {error}");
    }
}

#[cfg(not(feature = "backend-postgres"))]
#[tokio::test]
async fn config_check_rejects_postgres_schemes_when_driver_is_missing() {
    for scheme in ["postgres", "postgresql"] {
        let dir = TempDir::new().unwrap();
        let config_file = dir.path().join("pnpr.yaml");
        std::fs::write(
            &config_file,
            format!("database:\n  url: {scheme}://example:secret@database.example.test/registry\n"),
        )
        .unwrap();
        let error = check_config(Some(&config_file)).unwrap_err().to_string();
        assert!(error.contains("database driver"), "{scheme}: {error}");
        assert!(!error.contains("secret"), "{scheme}: {error}");
        let config = Config::from_yaml(&config_file, "127.0.0.1:0".parse().unwrap(), None).unwrap();
        assert_eq!(pnpr::serve(config).await.unwrap_err().to_string(), error);
    }
}

#[cfg(not(feature = "backend-mysql"))]
#[tokio::test]
async fn config_check_rejects_mysql_scheme_when_driver_is_missing() {
    let dir = TempDir::new().unwrap();
    let config_file = dir.path().join("pnpr.yaml");
    std::fs::write(
        &config_file,
        "database:\n  url: mysql://example:secret@database.example.test/registry\n",
    )
    .unwrap();
    let error = check_config(Some(&config_file)).unwrap_err().to_string();
    assert!(error.contains("database driver"), "{error}");
    assert!(!error.contains("secret"), "{error}");
    let config = Config::from_yaml(&config_file, "127.0.0.1:0".parse().unwrap(), None).unwrap();
    assert_eq!(pnpr::serve(config).await.unwrap_err().to_string(), error);
}
