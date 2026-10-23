// Example of using a offline writes with libSQL.

use libsql::{params, Builder};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // The local database path where the data will be stored.
    let db_path = std::env::var("LIBSQL_DB_PATH")
        .map_err(|_| {
            eprintln!(
                "Please set the LIBSQL_DB_PATH environment variable to set to local database path."
            )
        })
        .unwrap();

    // The remote sync URL to use.
    let sync_url = std::env::var("LIBSQL_SYNC_URL")
        .map_err(|_| {
            eprintln!(
                "Please set the LIBSQL_SYNC_URL environment variable to set to remote sync URL."
            )
        })
        .unwrap();

    // The authentication token to use.
    let auth_token = std::env::var("LIBSQL_AUTH_TOKEN").unwrap_or("".to_string());

    let db_builder = Builder::new_synced_database(db_path, sync_url, auth_token);

    let db = match db_builder.build().await {
        Ok(db) => db,
        Err(error) => {
            eprintln!("Error connecting to remote sync server: {}", error);
            return;
        }
    };

    let conn = db.connect().unwrap();

    println!("Syncing database from remote...");
    db.sync().await.unwrap();

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
    let mut results = conn
        .query("SELECT * FROM guest_book_entries", ())
        .await
        .unwrap();
    println!("Guest book entries:");
    while let Some(row) = results.next().await.unwrap() {
        let text: String = row.get(0).unwrap();
        println!("  {}", text);
    }

    println!("Syncing database to remote...");
    db.sync().await.unwrap();
    println!("Done!");
}
