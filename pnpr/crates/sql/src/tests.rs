#[cfg(feature = "backend-mysql")]
use super::mysql_pool;
#[cfg(feature = "backend-postgres")]
use super::postgres_pool;
use super::{Arc, Duration, SqlBackendSettings};

fn settings(url: &str) -> SqlBackendSettings {
    SqlBackendSettings {
        url: url.to_string(),
        max_connections: Some(2),
        timeout: Duration::from_secs(5),
        startup_timeout: Duration::from_secs(30),
    }
}

#[cfg(feature = "backend-postgres")]
#[tokio::test]
async fn postgres_callers_share_the_pool_and_its_limit() {
    let mut config = settings("postgres://example:secret@localhost/example");
    let first = postgres_pool(&config).unwrap();
    let second = postgres_pool(&config).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    config.max_connections = Some(3);
    assert!(!Arc::ptr_eq(&first, &postgres_pool(&config).unwrap()));
    config.max_connections = Some(0);
    assert!(postgres_pool(&config).is_err());
}

#[cfg(feature = "backend-mysql")]
#[tokio::test]
async fn mysql_callers_share_the_pool_and_its_limit() {
    let mut config = settings("mysql://example:secret@localhost/example");
    let first = mysql_pool(&config).unwrap();
    let second = mysql_pool(&config).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    config.max_connections = Some(3);
    assert!(!Arc::ptr_eq(&first, &mysql_pool(&config).unwrap()));
    config.max_connections = Some(0);
    assert!(mysql_pool(&config).is_err());
}

#[cfg(feature = "backend-postgres")]
#[tokio::test]
async fn startup_connection_failure_names_the_target_and_cause_without_credentials() {
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket
        .bind("127.0.0.1:0".parse().unwrap())
        .unwrap();
    let address = socket.local_addr().unwrap();
    let config = settings(&format!("postgres://example:example-secret@{address}/registry"));
    let pool = postgres_pool(&config).unwrap();
    let timeout = Duration::from_millis(50);
    let started = std::time::Instant::now();
    let error = super::connect_startup(&pool, &config.url, timeout).await.unwrap_err().to_string();
    assert!(started.elapsed() >= timeout);
    assert!(error.contains(&address.to_string()), "{error}");
    let cause = error.to_lowercase();
    assert!(cause.contains("refused") || cause.contains("timed out"), "{error}");
    assert!(!error.contains("example-secret"), "{error}");
}
