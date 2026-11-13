use super::{NetworkDialect, Param};
use crate::{Dialect, schema};

#[test]
fn placeholders_bind_in_occurrence_order_and_leave_literals_alone() {
    let params = [Param::Text("one"), Param::Integer(2)];
    for (dialect, expected) in [
        (NetworkDialect::Postgres, "SELECT $1, '?1', $2, 'it''s ?2', $3"),
        (NetworkDialect::Mysql, "SELECT ?, '?1', ?, 'it''s ?2', ?"),
    ] {
        let (sql, bound) =
            dialect.statement("SELECT ?2, '?1', ?1, 'it''s ?2', ?2", &params).unwrap();
        assert_eq!(sql, expected);
        assert_eq!(bound, [params[1], params[0], params[1]]);
    }
}

#[test]
fn dialect_schemas_have_the_same_tables_columns_and_keys() {
    let original = schema_contract(schema::TABLES.sqlite);
    for dialect in [Dialect::Postgres, Dialect::Mysql] {
        assert_eq!(schema_contract(schema::TABLES.get(dialect)), original, "{dialect:?}");
    }
}

#[test]
fn the_mysql_marker_table_carries_the_init_comment() {
    let comment = format!("COMMENT='{}'", super::super::mysql_schema::INIT_COMMENT);
    assert!(schema::MARKER_TABLE.mysql.ends_with(&comment));
}

/// The tables, columns, keys, and indexes `sql` creates, with column types
/// and collations mapped to `SQLite`'s. A partial index compares by its columns alone,
/// because `MySQL` has none.
fn schema_contract(sql: &str) -> Vec<String> {
    crate::schema::statements(sql)
        .filter(|statement| statement.starts_with("CREATE "))
        .map(|statement| {
            let statement = statement.replace(" IF NOT EXISTS", "");
            let end = statement.rfind(')').unwrap();
            statement[..=end]
                .replace("VARBINARY(512)", "TEXT")
                .replace("VARCHAR(512)", "TEXT")
                .replace(" CHARACTER SET utf8mb4 COLLATE utf8mb4_bin", "")
                .replace("VARCHAR(4096)", "TEXT")
                .replace("LONGTEXT", "TEXT")
                .replace("LONGBLOB", "BLOB")
                .replace("BYTEA", "BLOB")
                .replace("BIGINT", "INTEGER")
                .replace(r#" COLLATE "C""#, "")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}
