#[tokio::test]
#[ignore = "requires PNPR_TEST_MYSQL_URL"]
async fn auth_writes_keep_the_request_pool_deadline() {
    let settings = super::super::tests::saturated_pool_settings("PNPR_TEST_MYSQL_URL");
    let pool = pnpr_sql::mysql_pool(&settings).unwrap();
    let _held = pool.acquire().await.unwrap();
    let database = super::MysqlDatabase { pool };
    super::super::tests::assert_saturated_auth_writes_timeout(database).await;
}
