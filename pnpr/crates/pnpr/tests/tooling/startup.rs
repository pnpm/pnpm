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

#[cfg(feature = "backend-postgres")]
#[tokio::test]
async fn postgres_startup_retries_report_the_redacted_target_and_cause() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let log = tempfile::NamedTempFile::new().unwrap();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(std::sync::Arc::new(log.reopen().unwrap()))
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let settings = pnpr_config::SqlBackendSettings {
        url: format!("postgres://example:example-secret@{address}/registry"),
        max_connections: None,
        timeout: std::time::Duration::from_secs(1),
        startup_timeout: std::time::Duration::from_millis(400),
    };
    let backend = pnpr_config::BackendConfig::Postgres(settings);
    let error =
        pnpr_auth::AuthState::load(&Default::default(), &backend).await.unwrap_err().to_string();
    let output = std::fs::read_to_string(log.path()).unwrap();
    assert!(output.contains("database connection failed, retrying"), "{output}");
    for message in [&output, &error] {
        assert!(message.contains(&address.to_string()), "{message}");
        assert!(message.to_lowercase().contains("refused"), "{message}");
        assert!(!message.contains("example-secret"), "{message}");
    }
}
