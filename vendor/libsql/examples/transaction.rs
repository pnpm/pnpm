use libsql::Builder;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let db_file = tempfile::NamedTempFile::new().unwrap();
    println!("Database {}", db_file.path().display());

    let auth_token = std::env::var("TURSO_AUTH_TOKEN").unwrap_or_else(|_| {
        println!("Using empty token since TURSO_AUTH_TOKEN was not set");
        "".to_string()
    });

    let db = Builder::new_remote_replica(
        db_file.path(),
        "http://localhost:8080".to_string(),
        auth_token,
    )
    .build()
    .await
    .unwrap();

    db.sync().await.unwrap();

    let conn = db.connect().unwrap();

    conn.execute("BEGIN READONLY", ()).await.unwrap();
    conn.query("SELECT 1", ()).await.unwrap();
    conn.query("COMMIT", ()).await.unwrap();

    let tx = conn
        .transaction_with_behavior(libsql::TransactionBehavior::Immediate)
        .await
        .unwrap();

    tx.execute("INSERT INTO foo (x) VALUES (?1)", ["hello world"])
        .await
        .unwrap();

    tx.commit().await.unwrap();
}
