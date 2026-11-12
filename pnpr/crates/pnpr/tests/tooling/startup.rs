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

#[tokio::test]
async fn config_check_and_serve_reject_s3_or_populated_cache_without_secret() {
    let s3_dir = TempDir::new().unwrap();
    let s3_cfg = s3_dir.path().join("pnpr.yaml");
    std::fs::write(
        &s3_cfg,
        format!(
            "storage: '{}'\ncache: '{}'\ns3:\n  bucket: packages\n  region: auto\n  endpoint: https://storage.example.test\nregistries:\n  npm:\n    type: upstream\n    url: https://registry.npmjs.org/\n    public: true\ndefaultRegistry: npm\n",
            s3_dir.path().join("hosted").display(),
            s3_dir.path().join("cache").display(),
        ),
    )
    .unwrap();
    let s3_err = check_config(Some(&s3_cfg)).unwrap_err().to_string();
    assert!(s3_err.contains("`secret` or `secrets.cache` must be configured"), "{s3_err}");
    let config = Config::from_yaml(&s3_cfg, "127.0.0.1:0".parse().unwrap(), None).unwrap();
    assert_eq!(pnpr::serve(config).await.unwrap_err().to_string(), s3_err);

    let persisted_dir = TempDir::new().unwrap();
    let cache_dir = persisted_dir.path().join("cache");
    std::fs::create_dir_all(cache_dir.join("resolver-cache")).unwrap();
    let persisted_cfg = persisted_dir.path().join("pnpr.yaml");
    std::fs::write(
        &persisted_cfg,
        format!(
            "storage: '{}'\ncache: '{}'\n",
            persisted_dir
                .path()
                .join("hosted")
                .display(),
            cache_dir.display(),
        ),
    )
    .unwrap();
    let persisted_err = check_config(Some(&persisted_cfg)).unwrap_err().to_string();
    assert!(
        persisted_err.contains("when the cache directory persists across restarts"),
        "{persisted_err}",
    );
    let config = Config::from_yaml(&persisted_cfg, "127.0.0.1:0".parse().unwrap(), None).unwrap();
    assert_eq!(pnpr::serve(config).await.unwrap_err().to_string(), persisted_err);
}
