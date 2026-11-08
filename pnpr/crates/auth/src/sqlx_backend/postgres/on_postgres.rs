#[tokio::test]
#[ignore = "requires PNPR_TEST_POSTGRES_URL"]
async fn auth_writes_keep_the_request_pool_deadline() {
    let settings = super::super::tests::saturated_pool_settings("PNPR_TEST_POSTGRES_URL");
    let pool = pnpr_sql::postgres_pool(&settings).unwrap();
    let _held = pool.acquire().await.unwrap();
    let database =
        super::PostgresDatabase { pool: super::RequestPool::new(pool, settings.timeout) };
    super::super::tests::assert_saturated_auth_writes_timeout(database).await;
}

#[tokio::test]
#[ignore = "requires PNPR_TEST_POSTGRES_URL"]
async fn user_and_token_writes_share_one_connection() {
    let mut settings = super::super::tests::saturated_pool_settings("PNPR_TEST_POSTGRES_URL");
    settings.timeout = std::time::Duration::from_secs(2);
    let admin = pnpr_sql::postgres_pool(&settings).unwrap();
    let directory = tempfile::TempDir::new().unwrap();
    let database = super::super::tests::test_database_name(&directory);
    sqlx::query(&format!("CREATE DATABASE {database}")).execute(&*admin).await.unwrap();
    settings.url = format!("{}/{database}", settings.url.rsplit_once('/').unwrap().0);
    let pool = pnpr_sql::postgres_pool(&settings).unwrap();
    let auth = super::PostgresAuth::connect(&settings, pnpr_config::MaxUsers::Unlimited)
        .await
        .unwrap();
    super::super::tests::assert_network_auth_roundtrip(auth).await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {database}")).execute(&*admin).await.unwrap();
}
