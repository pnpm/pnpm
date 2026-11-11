use super::{Method, StatusCode, TempDir, app_with_token, record, signed};
use std::time::Duration;
use tower::ServiceExt as _;

#[tokio::test]
async fn cached_authentication_does_not_bypass_a_newer_database_schema() {
    let temp = TempDir::new().unwrap();
    let app = app_with_token(&temp, "test-token", record(false, &[]));
    let response = app
        .clone()
        .oneshot(signed(Method::GET, "/-/whoami", "test-token"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let database = pnpr_config::default_metadata_path(temp.path());
    let connection = rusqlite::Connection::open(database).unwrap();
    connection
        .execute(
            "INSERT INTO schema_migrations (version, applied_at) SELECT MAX(version) + 1, 1 FROM schema_migrations",
            [],
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let response = app
                .clone()
                .oneshot(signed(Method::GET, "/-/whoami", "test-token"))
                .await
                .unwrap();
            if response.status() == StatusCode::SERVICE_UNAVAILABLE {
                break;
            }
            assert_eq!(response.status(), StatusCode::OK);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    for path in ["/-/live", "/~local/-/live"] {
        let response = app
            .clone()
            .oneshot(signed(Method::GET, path, "test-token"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    for path in ["/-/ready", "/~local/-/ready"] {
        let response = app
            .clone()
            .oneshot(signed(Method::GET, path, "test-token"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
