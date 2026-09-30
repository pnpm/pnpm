use super::mature_version_or_running;
use pnpm_config::Config;

#[tokio::test]
async fn a_failed_release_age_lookup_records_the_running_pnpm() {
    let mut server = mockito::Server::new_async().await;
    let packument = server
        .mock("GET", "/pnpm")
        .with_status(500)
        .expect_at_least(1)
        .create_async()
        .await;
    let cache_dir = tempfile::TempDir::new().expect("cache tempdir");
    let mut config = Config {
        minimum_release_age: Some(24 * 60),
        fetch_retries: 0,
        cache_dir: cache_dir.path().to_path_buf(),
        ..Config::default()
    };
    config.package_manager_bootstrap.registry = format!("{}/", server.url());

    let version = mature_version_or_running(&config, "^1.0.0", "1.1.0".to_string()).await;

    assert_eq!(version, "1.1.0");
    packument.assert_async().await;
}
