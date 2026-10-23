use libsql::Builder;

#[tokio::main]
async fn main() {
    let db = if let Ok(url) = std::env::var("LIBSQL_URL") {
        let token = std::env::var("LIBSQL_AUTH_TOKEN").unwrap_or_else(|_| {
            println!("LIBSQL_TOKEN not set, using empty token...");
            "".to_string()
        });

        Builder::new_remote(url, token).build().await.unwrap()
    } else {
        Builder::new_local(":memory:").build().await.unwrap()
    };

    let conn = db.connect().unwrap();

    conn.query("select 1; select 1;", ()).await.unwrap();

    conn.execute("CREATE TABLE IF NOT EXISTS users (email TEXT)", ())
        .await
        .unwrap();

    let stmt = conn
        .prepare("INSERT INTO users (email) VALUES (?1)")
        .await
        .unwrap();

    stmt.execute(["foo@example.com"]).await.unwrap();

    let stmt = conn
        .prepare("SELECT * FROM users WHERE email = ?1")
        .await
        .unwrap();

    let mut rows = stmt.query(["foo@example.com"]).await.unwrap();

    let row = rows.next().await.unwrap().unwrap();

    let value = row.get_value(0).unwrap();

    println!("Row: {:?}", value);
}
