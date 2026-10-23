use libsql::{params, Builder};
use libsql_sys::{Cipher, EncryptionConfig};

#[tokio::test]
#[cfg(feature = "encryption")]
async fn test_encryption() {
    let tempdir = std::env::temp_dir();
    let encrypted_path = tempdir.join("encrypted.db");
    let base_path = tempdir.join("base.db");

    // lets create an encrypted database
    {
        let mut db_builder = Builder::new_local(&encrypted_path);
        db_builder = db_builder.encryption_config(EncryptionConfig {
            cipher: Cipher::Aes256Cbc,
            encryption_key: "s3cR3t".into(),
        });
        let db = db_builder.build().await.unwrap();

        let conn = db.connect().unwrap();
        conn.execute("CREATE TABLE IF NOT EXISTS messages (text TEXT)", ())
            .await
            .unwrap();
        let params = params!["the only winning move is not to play"];
        conn.execute("INSERT INTO messages (text) VALUES (?)", params)
            .await
            .unwrap();
    }

    // lets test encryption with ATTACH
    {
        let db = Builder::new_local(&base_path).build().await.unwrap();
        let conn = db.connect().unwrap();
        let attach_stmt = format!(
            "ATTACH DATABASE '{}' AS encrypted KEY 's3cR3t'",
            tempdir.join("encrypted.db").display()
        );
        conn.execute(&attach_stmt, ()).await.unwrap();
        let mut attached_results = conn
            .query("SELECT * FROM encrypted.messages", ())
            .await
            .unwrap();
        let row = attached_results.next().await.unwrap().unwrap();
        let text: String = row.get(0).unwrap();
        assert_eq!(text, "the only winning move is not to play");
    }

    {
        let _ = std::fs::remove_file(&encrypted_path);
        let _ = std::fs::remove_file(&base_path);
    }
}
