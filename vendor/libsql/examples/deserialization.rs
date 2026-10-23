use libsql::{de, Builder};

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

    conn.execute(
        "CREATE TABLE users (name TEXT, age INTEGER, vision FLOAT, avatar BLOB)",
        (),
    )
    .await
    .unwrap();

    let stmt = conn
        .prepare("INSERT INTO users (name, age, vision, avatar) VALUES (?1, ?2, ?3, ?4)")
        .await
        .unwrap();
    stmt.execute(("Ferris the Crab", 8, -6.5, vec![1, 2, 3]))
        .await
        .unwrap();

    let stmt = conn
        .prepare("SELECT * FROM users WHERE name = ?1")
        .await
        .unwrap();
    let row = stmt
        .query(["Ferris the Crab"])
        .await
        .unwrap()
        .next()
        .await
        .unwrap()
        .unwrap();

    #[derive(Debug, serde::Deserialize)]
    #[allow(dead_code)]
    struct User {
        name: String,
        age: i64,
        vision: f64,
        avatar: Vec<u8>,
    }

    let user = de::from_row::<User>(&row).unwrap();

    println!("User: {:?}", user);
}
