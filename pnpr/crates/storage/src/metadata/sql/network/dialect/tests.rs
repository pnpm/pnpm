use super::{Dialect, Param};

#[test]
fn placeholders_bind_in_occurrence_order_and_leave_literals_alone() {
    let params = [Param::Text("one"), Param::Integer(2)];
    for (dialect, expected) in [
        (Dialect::Postgres, "SELECT $1, '?1', $2, 'it''s ?2', $3"),
        (Dialect::Mysql, "SELECT ?, '?1', ?, 'it''s ?2', ?"),
    ] {
        let (sql, bound) =
            dialect.statement("SELECT ?2, '?1', ?1, 'it''s ?2', ?2", &params).unwrap();
        assert_eq!(sql, expected);
        assert_eq!(bound, [params[1], params[0], params[1]]);
    }
}

#[test]
fn mysql_upserts_do_not_ignore_other_constraint_errors() {
    let (sql, _) = Dialect::Mysql
        .statement(
            "INSERT INTO records (name, body) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
            &[Param::Text("example"), Param::Blob(b"payload")],
        )
        .unwrap();
    assert_eq!(
        sql,
        "INSERT INTO records (name, body) VALUES (?, ?)  ON DUPLICATE KEY UPDATE name = records.name",
    );
}

#[test]
fn mysql_conditional_upserts_keep_both_values_when_the_condition_fails() {
    let (sql, _) = Dialect::Mysql.statement(
        "INSERT INTO records (name, body) VALUES (?1, ?2) ON CONFLICT (name) DO UPDATE SET body = excluded.body, name = excluded.name WHERE records.name = excluded.name",
        &[Param::Text("example"), Param::Blob(b"payload")],
    ).unwrap();
    let sql = sql
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        sql.contains("body = IF( records.name = excluded.name, excluded.body, records.body)"),
        "{sql}",
    );
    assert!(sql.contains("AS excluded ON DUPLICATE KEY UPDATE"), "{sql}");
    assert!(!sql.contains(" WHERE "), "{sql}");
}

#[test]
fn dialect_schemas_have_the_same_tables_columns_and_keys() {
    let original = schema_contract(crate::metadata::sql::schema::TABLES);
    for dialect in [Dialect::Postgres, Dialect::Mysql] {
        assert_eq!(schema_contract(&dialect.tables()), original, "{dialect:?}");
    }
}

fn schema_contract(sql: &str) -> Vec<String> {
    crate::metadata::sql::schema::statements(sql)
        .filter(|statement| statement.starts_with("CREATE TABLE"))
        .map(|statement| {
            let end = statement.rfind(')').unwrap();
            statement[..=end]
                .replace("VARBINARY(512)", "TEXT")
                .replace("LONGTEXT", "TEXT")
                .replace("LONGBLOB", "BLOB")
                .replace("BYTEA", "BLOB")
                .replace("BIGINT", "INTEGER")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

#[test]
fn mysql_upserts_accept_columns_adjacent_to_the_table() {
    let (sql, _) = Dialect::Mysql
        .statement(
            "INSERT INTO records(name, body) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
            &[Param::Text("example"), Param::Blob(b"payload")],
        )
        .unwrap();
    assert!(sql.ends_with("name = records.name"), "{sql}");
}
