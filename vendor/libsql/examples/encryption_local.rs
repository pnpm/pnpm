// Example of showing using an encrypted local database with libsql. It also shows how to
// attach another encrypted database. The example expects a local `world.db` encrypted database
// to be present in the same directory.

use libsql::{params, Builder};
use libsql::{Cipher, EncryptionConfig};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // The local database path where the data will be stored.
    let db_path = std::env::var("LIBSQL_DB_PATH").unwrap();
    // The encryption key for the database.
    let encryption_key = std::env::var("LIBSQL_ENCRYPTION_KEY").unwrap_or("s3cR3t".to_string());

    let mut db_builder = Builder::new_local(db_path);

    db_builder = db_builder.encryption_config(EncryptionConfig {
        cipher: Cipher::Aes256Cbc,
        encryption_key: encryption_key.into(),
    });

    let db = db_builder.build().await.unwrap();
    let conn = db.connect().unwrap();
    conn.execute(
        "CREATE TABLE IF NOT EXISTS guest_book_entries (text TEXT)",
        (),
    )
    .await
    .unwrap();

    // let's attach another encrypted database and print its contents
    conn.execute("ATTACH DATABASE 'world.db' AS world KEY s3cR3t", ())
        .await
        .unwrap();

    let mut attached_results = conn
        .query("SELECT * FROM world.guest_book_entries", ())
        .await
        .unwrap();

    println!("attached database guest book entries:");
    while let Some(row) = attached_results.next().await.unwrap() {
        let text: String = row.get(0).unwrap();
        println!("  {}", text);
    }

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
