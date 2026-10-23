// Example of using offline writes with encryption

use libsql::{params, Builder};
use libsql::{EncryptionContext, EncryptionKey};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // The local database path where the data will be stored.
    let db_path = std::env::var("LIBSQL_DB_PATH").unwrap();

    // The remote sync URL to use.
    let sync_url = std::env::var("LIBSQL_SYNC_URL").unwrap();

    // The authentication token for the remote sync server.
    let auth_token = std::env::var("LIBSQL_AUTH_TOKEN").unwrap_or("".to_string());

    // Optional encryption key for the database, if provided.
    let encryption = if let Ok(key) = std::env::var("LIBSQL_ENCRYPTION_KEY") {
        Some(EncryptionContext {
            key: EncryptionKey::Base64Encoded(key),
        })
    } else {
        None
    };

    let mut db_builder = Builder::new_synced_database(db_path, sync_url, auth_token);

    if let Some(enc) = encryption {
        db_builder = db_builder.remote_encryption(enc);
    }

    let db = match db_builder.build().await {
        Ok(db) => db,
        Err(error) => {
            eprintln!("Error connecting to remote sync server: {}", error);
            return;
        }
    };

    let conn = db.connect().unwrap();

    print!("Syncing with remote database...");
    db.sync().await.unwrap();
    println!(" done");

    let mut results = conn.query("SELECT count(*) FROM dummy", ()).await.unwrap();
    let count: u32 = results.next().await.unwrap().unwrap().get(0).unwrap();
    println!("dummy table has {} entries", count);

    conn.execute(
        r#"
        CREATE TABLE IF NOT EXISTS guest_book_entries (
            text TEXT
        )"#,
        (),
    )
    .await
    .unwrap();

    let mut input = String::new();
    println!("Please write your entry to the guestbook:");
    match std::io::stdin().read_line(&mut input) {
        Ok(_) => {
            println!("You entered: {}", input);
            let params = params![input.as_str()];
            conn.execute("INSERT INTO guest_book_entries (text) VALUES (?)", params)
                .await
                .unwrap();
        }
        Err(error) => {
            eprintln!("Error reading input: {}", error);
        }
    }
    db.sync().await.unwrap();
    let mut results = conn
        .query("SELECT * FROM guest_book_entries", ())
        .await
        .unwrap();
    println!("Guest book entries:");
    while let Some(row) = results.next().await.unwrap() {
        let text: String = row.get(0).unwrap();
        println!("  {}", text);
    }
}
