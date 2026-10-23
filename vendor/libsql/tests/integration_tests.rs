#![allow(deprecated)]

use futures::{StreamExt, TryStreamExt};
use libsql::{
    named_params, params,
    params::{IntoParams, IntoValue},
    AuthAction, Authorization, Connection, Database, Op, Result, Value,
};
use rand::distributions::Uniform;
use rand::prelude::*;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

async fn setup() -> Connection {
    let db = Database::open(":memory:").unwrap();
    let conn = db.connect().unwrap();
    let _ = conn
        .execute("CREATE TABLE users (id INTEGER, name TEXT)", ())
        .await;
    conn
}

#[tokio::test]
async fn enable_disable_extension() {
    let db = Database::open(":memory:").unwrap();
    let conn = db.connect().unwrap();
    conn.load_extension_enable().unwrap();
    conn.load_extension_disable().unwrap();
}

#[tokio::test]
async fn add_update_hook() {
    let conn = setup().await;

    #[derive(PartialEq, Debug)]
    struct Data {
        op: Op,
        db: String,
        table: String,
        row_id: i64,
    }

    let d = Arc::new(Mutex::new(None::<Data>));

    let d_clone = d.clone();
    conn.add_update_hook(Box::new(move |op, db, table, row_id| {
        *d_clone.lock().unwrap() = Some(Data {
            op,
            db: db.to_string(),
            table: table.to_string(),
            row_id,
        });
    }))
    .unwrap();

    let _ = conn
        .execute("INSERT INTO users (id, name) VALUES (2, 'Alice')", ())
        .await
        .unwrap();

    assert_eq!(
        *d.lock().unwrap().as_ref().unwrap(),
        Data {
            op: Op::Insert,
            db: "main".to_string(),
            table: "users".to_string(),
            row_id: 1,
        }
    );

    let _ = conn
        .execute("UPDATE users SET name = 'Bob' WHERE id = 2", ())
        .await
        .unwrap();

    assert_eq!(
        *d.lock().unwrap().as_ref().unwrap(),
        Data {
            op: Op::Update,
            db: "main".to_string(),
            table: "users".to_string(),
            row_id: 1,
        }
    );

    let _ = conn
        .execute("DELETE FROM users WHERE id = 2", ())
        .await
        .unwrap();

    assert_eq!(
        *d.lock().unwrap().as_ref().unwrap(),
        Data {
            op: Op::Delete,
            db: "main".to_string(),
            table: "users".to_string(),
            row_id: 1,
        }
    );
}

#[tokio::test]
async fn connection_drops_before_statements() {
    let db = Database::open(":memory:").unwrap();
    let conn = db.connect().unwrap();
    let _stmt = conn.prepare("SELECT 1").await.unwrap();
    drop(conn);
}

#[tokio::test]
async fn file_prefix_open() {
    let tempdir = std::env::temp_dir();

    let path = tempdir.join("prefixeddata.db");

    let path = format!("file:{}", path.display());

    let db = Database::open(path).unwrap();
    let conn = db.connect().unwrap();
    let _stmt = conn.prepare("SELECT 1").await.unwrap();
    drop(conn);
}

#[tokio::test]
async fn connection_query() {
    let conn = setup().await;
    let _ = conn
        .execute("INSERT INTO users (id, name) VALUES (2, 'Alice')", ())
        .await
        .unwrap();
    let mut rows = conn.query("SELECT * FROM users", ()).await.unwrap();
    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<i32>(0).unwrap(), 2);
    assert_eq!(row.get::<String>(1).unwrap(), "Alice");
}

#[tokio::test]
async fn connection_execute_transactional_batch_success() {
    let conn = setup().await;

    conn.execute_transactional_batch(
        "CREATE TABLE foo(x INTEGER);
         CREATE TABLE bar(y TEXT);",
    )
    .await
    .unwrap();

    let mut rows = conn
        .query(
            "SELECT
                name
            FROM
                sqlite_schema
            WHERE
                type ='table' AND
                name NOT LIKE 'sqlite_%';",
            (),
        )
        .await
        .unwrap();

    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<String>(0).unwrap(), "users");

    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<String>(0).unwrap(), "foo");

    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<String>(0).unwrap(), "bar");

    assert!(rows.next().await.unwrap().is_none());
}

#[tokio::test]
async fn connection_execute_transactional_batch_fail() {
    let conn = setup().await;

    let res = conn
        .execute_transactional_batch(
            "CREATE TABLE unexpected_foo(x INTEGER);
            CREATE TABLE sqlite_schema(y TEXT);
         CREATE TABLE unexpected_bar(y TEXT);",
        )
        .await;
    assert!(res.is_err());

    let mut rows = conn
        .query(
            "SELECT
                name
            FROM
                sqlite_schema
            WHERE
                type ='table' AND
                name NOT LIKE 'sqlite_%';",
            (),
        )
        .await
        .unwrap();

    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<String>(0).unwrap(), "users");

    assert!(rows.next().await.unwrap().is_none());
}

#[tokio::test]
async fn connection_execute_transactional_batch_transaction_fail() {
    let conn = setup().await;

    let res = conn
        .execute_transactional_batch(
            "BEGIN;
        CREATE TABLE unexpected_foo(x INTEGER);
        COMMIT;
        CREATE TABLE sqlite_schema(y TEXT);
        CREATE TABLE unexpected_bar(y TEXT);",
        )
        .await;
    assert!(res.is_err());

    let mut rows = conn
        .query(
            "SELECT
                name
            FROM
                sqlite_schema
            WHERE
                type ='table' AND
                name NOT LIKE 'sqlite_%';",
            (),
        )
        .await
        .unwrap();

    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<String>(0).unwrap(), "users");

    assert!(rows.next().await.unwrap().is_none());
}

#[tokio::test]
async fn connection_execute_transactional_batch_transaction_incorrect() {
    let conn = setup().await;

    let res = conn
        .execute_transactional_batch(
            "COMMIT;
        CREATE TABLE unexpected_foo(x INTEGER);
        CREATE TABLE sqlite_schema(y TEXT);
        CREATE TABLE unexpected_bar(y TEXT);",
        )
        .await;
    assert!(res.is_err());

    let mut rows = conn
        .query(
            "SELECT
                name
            FROM
                sqlite_schema
            WHERE
                type ='table' AND
                name NOT LIKE 'sqlite_%';",
            (),
        )
        .await
        .unwrap();

    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<String>(0).unwrap(), "users");

    assert!(rows.next().await.unwrap().is_none());
}

#[tokio::test]
async fn connection_execute_batch() {
    let conn = setup().await;

    conn.execute_batch(
        "BEGIN;
         CREATE TABLE foo(x INTEGER);
         CREATE TABLE bar(y TEXT);
         COMMIT;",
    )
    .await
    .unwrap();

    let mut rows = conn
        .query(
            "SELECT 
                name
            FROM 
                sqlite_schema
            WHERE 
                type ='table' AND 
                name NOT LIKE 'sqlite_%';",
            (),
        )
        .await
        .unwrap();

    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<String>(0).unwrap(), "users");

    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<String>(0).unwrap(), "foo");

    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<String>(0).unwrap(), "bar");
}

#[tokio::test]
async fn connection_execute_batch_inserts() {
    let conn = setup().await;

    conn.execute("CREATE TABLE foo(x INTEGER)", ())
        .await
        .unwrap();

    conn.execute_batch(
        "BEGIN;
        INSERT INTO foo VALUES (1);
        INSERT INTO foo VALUES (2);
        INSERT INTO foo VALUES (3);
        COMMIT;
        ",
    )
    .await
    .unwrap();

    let mut rows = conn.query("SELECT count(*) FROM foo", ()).await.unwrap();

    let count = rows.next().await.unwrap().unwrap().get::<u64>(0).unwrap();
    assert_eq!(count, 3);
}

#[tokio::test]
async fn connection_execute_batch_inserts_returning() {
    let conn = setup().await;

    conn.execute("CREATE TABLE foo(x INTEGER)", ())
        .await
        .unwrap();

    let mut batch_rows = conn
        .execute_batch(
            "BEGIN;
            INSERT INTO foo VALUES (1) RETURNING *;
            INSERT INTO foo VALUES (2) RETURNING *;
            INSERT INTO foo VALUES (3) RETURNING *;
            COMMIT;
            ",
        )
        .await
        .unwrap();

    assert!(batch_rows.next_stmt_row().unwrap().is_none());

    let mut rows = batch_rows.next_stmt_row().unwrap().unwrap();
    assert_eq!(
        rows.next().await.unwrap().unwrap().get::<u64>(0).unwrap(),
        1
    );
    let mut rows = batch_rows.next_stmt_row().unwrap().unwrap();
    assert_eq!(
        rows.next().await.unwrap().unwrap().get::<u64>(0).unwrap(),
        2
    );

    let mut rows = batch_rows.next_stmt_row().unwrap().unwrap();
    assert_eq!(
        rows.next().await.unwrap().unwrap().get::<u64>(0).unwrap(),
        3
    );

    assert!(batch_rows.next_stmt_row().unwrap().is_none());
}

#[tokio::test]
async fn connection_execute_batch_newline() {
    // This test checks that we handle a null raw
    // stament in execute_batch. What happens when there
    // are no more queries but the sql string is not empty?
    // Well sqlite returns a null statment from prepare_v2
    // so this test checks that we check if the statement is
    // null before we try to step it!
    let conn = setup().await;

    conn.execute_batch(
        "
        create table foo (x INT);
        ",
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn prepare_invalid_sql() {
    let conn = setup().await;
    let result = conn.prepare("SYNTAX ERROR").await;
    assert!(result.is_err());
    let actual = result.err().unwrap();
    match actual {
        libsql::Error::SqliteFailure(code, msg) => {
            assert_eq!(code, 1);
            assert_eq!(msg, "near \"SYNTAX\": syntax error".to_string());
        }
        _ => panic!("Expected SqliteFailure"),
    }
}

#[tokio::test]
async fn statement_query() {
    let conn = setup().await;
    let _ = conn
        .execute("INSERT INTO users (id, name) VALUES (2, 'Alice')", ())
        .await
        .unwrap();

    let params = [2];

    let mut stmt = conn
        .prepare("SELECT * FROM users WHERE id = ?1")
        .await
        .unwrap();

    let mut rows = stmt.query(&params).await.unwrap();
    let row = rows.next().await.unwrap().unwrap();

    assert_eq!(row.get::<i32>(0).unwrap(), 2);
    assert_eq!(row.get::<String>(1).unwrap(), "Alice");

    stmt.reset();

    let row = stmt.query_row(&params).await.unwrap();

    assert_eq!(row.get::<i32>(0).unwrap(), 2);
    assert_eq!(row.get::<String>(1).unwrap(), "Alice");

    stmt.reset();

    let rows = stmt.query(&params).await.unwrap();
    let mut names = rows
        .into_stream()
        .boxed()
        .map_ok(|r| r.get::<String>(1).unwrap());

    let name = names.next().await.unwrap().unwrap();

    assert_eq!(name, "Alice");
}

#[tokio::test]
async fn prepare_and_query() {
    let conn = setup().await;
    check_insert(
        &conn,
        "INSERT INTO users (id, name) VALUES (2, 'Alice')",
        (),
    )
    .await;
    check_insert(
        &conn,
        "INSERT INTO users (id, name) VALUES (?1, ?2)",
        params![2u64, "Alice".to_string()],
    )
    .await;
    check_insert(
        &conn,
        "INSERT INTO users (id, name) VALUES (?1, ?2)",
        params![2, "Alice"],
    )
    .await;
}

#[tokio::test]
async fn prepare_and_query_named_params() {
    let conn = setup().await;

    conn.query("SELECT 1", named_params![]).await.unwrap();
    conn.query("SELECT 1", params![]).await.unwrap();
    conn.query("SELECT 1", ()).await.unwrap();

    check_insert(
        &conn,
        "INSERT INTO users (id, name) VALUES (:a, :b)",
        ((":a", 2), (":b", "Alice")),
    )
    .await;

    check_insert(
        &conn,
        "INSERT INTO users (id, name) VALUES (:a, :b)",
        named_params! {
            ":a": 2,
            ":b": "Alice"
        },
    )
    .await;

    check_insert(
        &conn,
        "INSERT INTO users (id, name) VALUES (:a, :b)",
        named_params! {
            ":a": 2,
            ":b": "Alice",
        },
    )
    .await;

    check_insert(
        &conn,
        "INSERT INTO users (id, name) VALUES (@a, @b)",
        named_params! {
            "@a": 2,
            "@b": "Alice",
        },
    )
    .await;

    check_insert(
        &conn,
        "INSERT INTO users (id, name) VALUES ($a, $b)",
        named_params! {
            "$a": 2,
            "$b": "Alice",
        },
    )
    .await;

    check_insert(
        &conn,
        "INSERT INTO users (id, name) VALUES ($a, $b)",
        named_params! {
            "$a": 2,
            "$b": "Alice",
        },
    )
    .await;
}

#[tokio::test]
async fn prepare_and_dont_query() {
    // TODO: how can we check that we've cleaned up the statement?

    let conn = setup().await;

    conn.prepare("INSERT INTO users (id, name) VALUES (?1, ?2)")
        .await
        .unwrap();

    // Drop the connection explicitly here to show that we want to drop
    // it while the above statment has not been queryd.
    drop(conn);
}

async fn check_insert(conn: &Connection, sql: &str, params: impl IntoParams) {
    let _ = conn.execute(sql, params).await.unwrap();
    let mut rows = conn.query("SELECT * FROM users", ()).await.unwrap();
    let row = rows.next().await.unwrap().unwrap();
    // Use two since if you forget to insert an id it will automatically
    // be set to 1 which defeats the purpose of checking it here.
    assert_eq!(row.get::<i32>(0).unwrap(), 2);
    assert_eq!(row.get::<String>(1).unwrap(), "Alice");
}

#[tokio::test]
async fn nulls() {
    let conn = setup().await;
    let _ = conn
        .execute("INSERT INTO users (id, name) VALUES (NULL, NULL)", ())
        .await
        .unwrap();
    let mut rows = conn.query("SELECT * FROM users", ()).await.unwrap();
    let row = rows.next().await.unwrap().unwrap();
    assert!(row.get::<i32>(0).is_err());
    assert!(row.get::<String>(1).is_err());
}

#[tokio::test]
async fn blob() {
    let conn = setup().await;
    let _ = conn
        .execute("CREATE TABLE bbb (id INTEGER PRIMARY KEY, data BLOB)", ())
        .await
        .unwrap();

    let bytes = vec![2u8; 64];
    let _ = conn
        .execute("INSERT INTO bbb (data) VALUES (?1)", [bytes.clone()])
        .await
        .unwrap();

    let mut rows = conn.query("SELECT * FROM bbb", ()).await.unwrap();
    let row = rows.next().await.unwrap().unwrap();

    let out = row.get::<Vec<u8>>(1).unwrap();
    assert_eq!(&out, &bytes);

    let empty: Vec<u8> = vec![];
    let mut rows = conn
        .query(
            "INSERT INTO bbb (data) VALUES (?1) RETURNING *",
            [Value::Blob(empty.clone())],
        )
        .await
        .unwrap();
    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<Vec<u8>>(1).unwrap(), empty);
}

#[tokio::test]
async fn transaction() {
    let conn = setup().await;
    conn.execute("INSERT INTO users (id, name) VALUES (2, 'Alice')", ())
        .await
        .unwrap();
    let tx = conn.transaction().await.unwrap();
    tx.execute("INSERT INTO users (id, name) VALUES (3, 'Bob')", ())
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    let mut rows = conn.query("SELECT * FROM users", ()).await.unwrap();
    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<i32>(0).unwrap(), 2);
    assert_eq!(row.get::<String>(1).unwrap(), "Alice");
    assert!(rows.next().await.unwrap().is_none());
}

#[tokio::test]
async fn custom_params() {
    let conn = setup().await;

    enum MyValue {
        Text(String),
        Int(i64),
    }

    impl IntoValue for MyValue {
        fn into_value(self) -> libsql::Result<Value> {
            match self {
                MyValue::Text(s) => Ok(Value::Text(s)),
                MyValue::Int(i) => Ok(Value::Integer(i)),
            }
        }
    }

    let params = vec![MyValue::Int(2), MyValue::Text("Alice".into())];

    conn.execute(
        "INSERT INTO users (id, name) VALUES (?1, ?2)",
        libsql::params_from_iter(params),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn debug_print_row() {
    let db = Database::open(":memory:").unwrap();
    let conn = db.connect().unwrap();
    let _ = conn
        .execute(
            "CREATE TABLE users (id INTEGER, name TEXT, score REAL, data BLOB, age INTEGER)",
            (),
        )
        .await;
    conn.execute("INSERT INTO users (id, name, score, data, age) VALUES (123, 'potato', 3.14, X'deadbeef', NULL)", ())
    .await
    .unwrap();

    let stmt = conn.prepare("SELECT * FROM users").await.unwrap();
    let mut rows = stmt.query(()).await.unwrap();
    assert_eq!(
        format!("{:?}", rows.next().await.unwrap().unwrap()),
        "{Some(\"id\"): (Integer, 123), Some(\"name\"): (Text, \"potato\"), Some(\"score\"): (Real, 3.14), Some(\"data\"): (Blob, 4), Some(\"age\"): (Null, ())}"
    );
}

#[tokio::test]
async fn fts5_invalid_tokenizer() {
    let db = Database::open(":memory:").unwrap();
    let conn = db.connect().unwrap();
    assert!(conn
        .execute(
            "CREATE VIRTUAL TABLE t USING fts5(s, tokenize='trigram case_sensitive ')",
            (),
        )
        .await
        .is_err());
    assert!(conn
        .execute(
            "CREATE VIRTUAL TABLE t USING fts5(s, tokenize='trigram remove_diacritics ')",
            (),
        )
        .await
        .is_err());
}

#[cfg(feature = "serde")]
#[tokio::test]
async fn deserialize_row() {
    let db = Database::open(":memory:").unwrap();
    let conn = db.connect().unwrap();
    let _ = conn
        .execute(
            "CREATE TABLE users (id INTEGER, name TEXT, score REAL, data BLOB, age INTEGER, status TEXT, wrapper TEXT)",
            (),
        )
        .await;
    conn.execute("INSERT INTO users (id, name, score, data, age, status, wrapper) VALUES (123, 'potato', 42.0, X'deadbeef', NULL, 'Draft', 'Published')", ())
    .await
    .unwrap();

    use serde::Deserialize;

    #[derive(Deserialize, Debug)]
    struct Data {
        id: i64,
        name: String,
        score: f64,
        data: Vec<u8>,
        age: Option<i64>,
        none: Option<()>,
        status: Status,
        wrapper: Wrapper,
    }

    #[derive(Deserialize, Debug, PartialEq)]
    enum Status {
        Draft,
        Published,
    }

    #[derive(Deserialize, Debug, PartialEq)]
    #[serde(transparent)]
    struct Wrapper(Status);

    let row = conn
        .query("SELECT * FROM users", ())
        .await
        .unwrap()
        .next()
        .await
        .unwrap()
        .unwrap();
    let data: Data = libsql::de::from_row(&row).unwrap();
    assert_eq!(data.id, 123);
    assert_eq!(data.name, "potato".to_string());
    assert_eq!(data.score, 42.0);
    assert_eq!(data.data, vec![0xde, 0xad, 0xbe, 0xef]);
    assert_eq!(data.age, None);
    assert_eq!(data.none, None);
    assert_eq!(data.status, Status::Draft);
    assert_eq!(data.wrapper, Wrapper(Status::Published));
}

#[tokio::test]
#[ignore]
// fuzz test can be run explicitly with following command:
// cargo test vector_fuzz_test -- --nocapture --include-ignored
async fn vector_fuzz_test() {
    let mut global_rng = rand::thread_rng();
    for attempt in 0..10000 {
        let seed = global_rng.next_u64();

        let mut rng =
            rand::rngs::StdRng::from_seed(unsafe { std::mem::transmute([seed, seed, seed, seed]) });
        let db = Database::open(":memory:").unwrap();
        let conn = db.connect().unwrap();
        let dim = rng.gen_range(1..=1536);
        let operations = rng.gen_range(1..128);
        println!(
            "============== ATTEMPT {} (seed {}u64, dim {}, operations {}) ================",
            attempt, seed, dim, operations
        );

        let _ = conn
            .execute(
                &format!(
                    "CREATE TABLE users (id INTEGER PRIMARY KEY, v FLOAT32({}) )",
                    dim
                ),
                (),
            )
            .await;
        // println!("CREATE TABLE users (id INTEGER PRIMARY KEY, v FLOAT32({}) );", dim);
        let _ = conn
            .execute(
                "CREATE INDEX users_idx ON users ( libsql_vector_idx(v) );",
                (),
            )
            .await;
        // println!("CREATE INDEX users_idx ON users ( libsql_vector_idx(v) );");

        let mut next_id = 1;
        let mut alive = HashSet::new();
        let uniform = Uniform::new(-1.0, 1.0);
        for _ in 0..operations {
            let operation = rng.gen_range(0..4);
            let vector: Vec<f32> = (0..dim).map(|_| rng.sample(uniform)).collect();
            let vector_str = format!(
                "[{}]",
                vector
                    .iter()
                    .map(|x| format!("{}", x))
                    .collect::<Vec<String>>()
                    .join(",")
            );
            if operation == 0 {
                // println!("INSERT INTO users VALUES ({}, vector('{}') );", next_id, vector_str);
                conn.execute(
                    "INSERT INTO users VALUES (?, vector(?) )",
                    libsql::params![next_id, vector_str],
                )
                .await
                .unwrap();
                alive.insert(next_id);
                next_id += 1;
            } else if operation == 1 {
                let id = rng.gen_range(0..next_id);
                // println!("DELETE FROM users WHERE id = {};", id);
                conn.execute("DELETE FROM users WHERE id = ?", libsql::params![id])
                    .await
                    .unwrap();
                alive.remove(&id);
            } else if operation == 2 && !alive.is_empty() {
                let id = alive.iter().collect::<Vec<_>>()[rng.gen_range(0..alive.len())];
                // println!("UPDATE users SET v = vector('{}') WHERE id = {};", vector_str, id);
                conn.execute(
                    "UPDATE users SET v = vector(?) WHERE id = ?",
                    libsql::params![vector_str, id],
                )
                .await
                .unwrap();
            } else if operation == 3 {
                let k = rng.gen_range(1..200);
                // println!("SELECT * FROM vector_top_k('users_idx', '{}', {});", vector_str, k);
                let result = conn
                    .query(
                        "SELECT * FROM vector_top_k('users_idx', ?, ?)",
                        libsql::params![vector_str, k],
                    )
                    .await
                    .unwrap();
                let count = result.into_stream().count().await;
                assert!(count <= alive.len());
                if alive.len() > 0 {
                    assert!(count > 0);
                }
            }
        }
        let _ = conn.execute("REINDEX users;", ()).await.unwrap();
    }
}

#[tokio::test]
async fn test_deny_authorizer() {
    let db = Database::open(":memory:").unwrap();
    let conn = db.connect().unwrap();
    conn.execute("CREATE TABLE users (id INTEGER, name TEXT)", ())
        .await
        .unwrap();
    conn.authorizer(Some(Arc::new(|ctx| {
        assert_eq!(
            ctx.action,
            AuthAction::Insert {
                table_name: "users"
            }
        );
        assert_eq!(ctx.database_name, Some("main"));
        assert_eq!(ctx.accessor, None);
        Authorization::Deny
    })))
    .unwrap();
    let res = conn
        .execute("INSERT INTO users (id, name) VALUES (1, 'Alice')", ())
        .await;
    assert_sqlite_error(res, libsql::ffi::SQLITE_AUTH);
    conn.authorizer(None).unwrap();
    conn.execute("INSERT INTO users (id, name) VALUES (1, 'Alice')", ())
        .await
        .unwrap();
}

#[tokio::test]
async fn test_ignore_authorizer() {
    let db = Database::open(":memory:").unwrap();
    let conn = db.connect().unwrap();
    conn.execute("CREATE TABLE users (id INTEGER, name TEXT)", ())
        .await
        .unwrap();
    conn.authorizer(Some(Arc::new(|ctx| {
        assert_eq!(
            ctx.action,
            AuthAction::Insert {
                table_name: "users"
            }
        );
        assert_eq!(ctx.database_name, Some("main"));
        assert_eq!(ctx.accessor, None);
        Authorization::Ignore
    })))
    .unwrap();
    conn.execute("INSERT INTO users (id, name) VALUES (1, 'Alice')", ())
        .await
        .unwrap();
    conn.authorizer(None).unwrap();
    let rows = conn.query("SELECT * FROM users", ()).await.unwrap();
    // There should be no rows
    assert_eq!(rows.into_stream().count().await, 0);
}

fn assert_sqlite_error<T>(res: Result<T>, code: i32) {
    match res {
        Ok(_) => panic!("Expected error, got Ok"),
        Err(e) => {
            if let libsql::Error::SqliteFailure(c, _) = e {
                assert!(c == code, "Expected error code {}, got {}", code, c);
            } else {
                panic!("Expected SqliteFailure, got {:?}", e);
            }
        }
    }
}
