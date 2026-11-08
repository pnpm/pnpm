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
