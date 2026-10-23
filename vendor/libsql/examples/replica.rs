use libsql::{Builder, Cipher, EncryptionConfig, Value};
use std::time::Duration;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let db_dir = tempfile::tempdir().unwrap();
    let db_file = db_dir.path().join("data.db");
    println!("Database {}", db_file.display());

    let auth_token = std::env::var("LIBSQL_AUTH_TOKEN").unwrap_or_else(|_| {
        println!("Using empty token since LIBSQL_TOKEN was not set");
        "".to_string()
    });

    let url = std::env::var("LIBSQL_URL")
        .unwrap_or_else(|_| {
            println!("Using http://localhost:8080 LIBSQL_URL was not set");
            "http://localhost:8080".to_string()
        })
        .replace("libsql", "https");

    let db = if cfg!(feature = "encryption") {
        let encryption_config = EncryptionConfig {
            cipher: Cipher::Aes256Cbc,
            encryption_key: "s3cr3t".into(),
        };
        Builder::new_remote_replica(&db_file, url, auth_token)
            .encryption_config(encryption_config)
            .build()
            .await
            .unwrap()
    } else {
        Builder::new_remote_replica(&db_file, url, auth_token)
            .build()
            .await
            .unwrap()
    };

    let conn = db.connect().unwrap();

    let f = db.sync().await.unwrap();
    println!("inital sync complete, frame no: {f:?}");

    conn.execute("CREATE TABLE IF NOT EXISTS foo (x TEXT)", ())
        .await
        .unwrap();

    db.sync().await.unwrap();

    let mut jh = tokio::spawn(async move {
        let mut rows = conn
            .query(
                "INSERT INTO foo (x) VALUES (?1) RETURNING *",
                vec![Value::from(
                    "this value was written by an embedded replica!",
                )],
            )
            .await
            .unwrap();

        println!("Rows insert call");
        while let Some(row) = rows.next().await.unwrap() {
            println!("Row: {}", row.get_str(0).unwrap());
        }

        println!("--------");

        let mut rows = conn.query("SELECT * FROM foo", ()).await.unwrap();

        println!("Rows coming from a read after write call");
        while let Some(row) = rows.next().await.unwrap() {
            println!("Row: {}", row.get_str(0).unwrap());
        }

        println!("--------");
    });

    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                let r = db.sync().await.unwrap();
                println!("replicated until index {r:?}");
            }

            r = &mut jh => {
                r.unwrap();
                return;
            }
        }
    }
}
