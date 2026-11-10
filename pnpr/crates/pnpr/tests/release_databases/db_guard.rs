use tempfile::TempDir;

pub struct TestDatabase {
    admin_url: String,
    db_name: String,
    is_postgres: bool,
    pub url: String,
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        let admin_url = self.admin_url.clone();
        let db_name = self.db_name.clone();
        let is_postgres = self.is_postgres;
        let runtime = tokio::runtime::Handle::current();
        let result = tokio::task::block_in_place(|| {
            runtime.block_on(drop_database(&admin_url, &db_name, is_postgres))
        });
        if let Err(err) = result
            && !std::thread::panicking()
        {
            panic!("failed to drop test database {db_name}: {err}");
        }
    }
}

async fn drop_database(admin_url: &str, db_name: &str, is_postgres: bool) -> sqlx::Result<()> {
    if is_postgres {
        let pool = sqlx::PgPool::connect(admin_url).await?;
        sqlx::query(&format!("DROP DATABASE {db_name} WITH (FORCE)")).execute(&pool).await?;
        pool.close().await;
    } else {
        let pool = sqlx::MySqlPool::connect(admin_url).await?;
        sqlx::query(&format!("DROP DATABASE {db_name}")).execute(&pool).await?;
        pool.close().await;
    }
    Ok(())
}

pub async fn create_postgres_database(admin_url: &str, scheme: &str) -> TestDatabase {
    let tmp = TempDir::new().unwrap();
    let db_name = unique_db_name("release_pg", &tmp);
    let pool = sqlx::PgPool::connect(admin_url).await.unwrap();
    sqlx::query(&format!("CREATE DATABASE {db_name}")).execute(&pool).await.unwrap();
    pool.close().await;

    let base = admin_url.rsplit_once('/').expect("postgres admin URL has database path").0;
    let rest = base.split_once("://").expect("postgres admin URL has scheme").1;
    TestDatabase {
        admin_url: admin_url.to_string(),
        url: format!("{scheme}://{rest}/{db_name}"),
        db_name,
        is_postgres: true,
    }
}

pub async fn create_mysql_database(admin_url: &str) -> TestDatabase {
    let tmp = TempDir::new().unwrap();
    let db_name = unique_db_name("release_my", &tmp);
    let pool = sqlx::MySqlPool::connect(admin_url).await.unwrap();
    sqlx::query(&format!("CREATE DATABASE {db_name}")).execute(&pool).await.unwrap();
    pool.close().await;

    let base = admin_url.rsplit_once('/').expect("mysql admin URL has database path").0;
    TestDatabase {
        admin_url: admin_url.to_string(),
        url: format!("{base}/{db_name}"),
        db_name,
        is_postgres: false,
    }
}

fn unique_db_name(prefix: &str, tmp: &TempDir) -> String {
    let suffix = tmp
        .path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .replace(['.', '-'], "")
        .to_ascii_lowercase();
    format!("{prefix}_{suffix}")
}
