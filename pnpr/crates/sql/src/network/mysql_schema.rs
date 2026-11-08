use super::super::{adapter::Connection, schema::MARKER};
use pnpr_error::Result;

/// The table comment placed on [`MARKER`] on `MySQL` before step 1 finishes.
const INIT_COMMENT: &str = "pnpr-init:1";

/// The statement that creates [`MARKER`] on `MySQL`, carrying the `pnpr-init:1`
/// comment so an interrupted first migration can resume.
pub const MARKER_TABLE: &str = "CREATE TABLE IF NOT EXISTS schema_migrations (version BIGINT NOT NULL PRIMARY KEY) ENGINE=InnoDB COMMENT='pnpr-init:1'";

pub(super) fn is_recoverable(connection: &dyn Connection) -> Result<bool> {
    let Some(mut row) = connection.query_optional(
        "SELECT table_comment FROM information_schema.tables WHERE table_schema = DATABASE() AND table_name = ?1",
        params![MARKER],
    )?
    else {
        return Ok(false);
    };
    let comment = row.get::<String>(0)?;
    Ok(comment == INIT_COMMENT)
}

pub(super) fn execute_ddl(connection: &dyn Connection, statement: &str) -> Result<()> {
    let trimmed = statement.trim();
    let upper = trimmed.to_ascii_uppercase();
    if upper.starts_with("CREATE INDEX ") || upper.starts_with("CREATE UNIQUE INDEX ") {
        let normalized = strip_if_not_exists(trimmed, "INDEX ");
        if index_exists(connection, &normalized)? {
            return Ok(());
        }
        return connection.execute(&normalized, params![]);
    }
    if upper.starts_with("ALTER TABLE ") && upper.contains(" ADD COLUMN ") {
        let normalized = strip_if_not_exists(trimmed, "ADD COLUMN ");
        if column_exists(connection, &normalized)? {
            return Ok(());
        }
        return connection.execute(&normalized, params![]);
    }
    connection.execute(trimmed, params![])
}

fn strip_if_not_exists(statement: &str, after_keyword: &str) -> String {
    let upper = statement.to_ascii_uppercase();
    let Some(keyword_pos) = upper.find(after_keyword) else {
        return statement.to_string();
    };
    let rest_start = keyword_pos + after_keyword.len();
    let rest_trimmed = statement[rest_start..].trim_start();
    if rest_trimmed.to_ascii_uppercase().starts_with("IF NOT EXISTS ") {
        let after_if = rest_trimmed["IF NOT EXISTS ".len()..].trim_start();
        format!("{}{after_if}", &statement[..rest_start])
    } else {
        statement.to_string()
    }
}

fn index_exists(connection: &dyn Connection, statement: &str) -> Result<bool> {
    let upper = statement.to_ascii_uppercase();
    let Some(index_pos) = upper.find("INDEX ") else {
        return Ok(false);
    };
    let rest = &statement[index_pos + "INDEX ".len()..];
    let rest_upper = rest.to_ascii_uppercase();
    let Some(on_pos) = rest_upper.find(" ON ") else {
        return Ok(false);
    };
    let name = rest[..on_pos].trim();
    let after_on = &rest[on_pos + " ON ".len()..];
    let table = after_on
        .split(|character: char| character.is_whitespace() || character == '(')
        .find(|token| !token.is_empty())
        .expect("static index table");
    Ok(connection
        .query_optional(
            "SELECT 1 FROM information_schema.statistics WHERE table_schema = DATABASE() AND table_name = ?1 AND index_name = ?2 LIMIT 1",
            params![table, name],
        )?
        .is_some())
}

fn column_exists(connection: &dyn Connection, statement: &str) -> Result<bool> {
    let upper = statement.to_ascii_uppercase();
    let after_alter = &statement["ALTER TABLE ".len()..];
    let after_alter_upper = &upper["ALTER TABLE ".len()..];
    let Some(add_pos) = after_alter_upper.find(" ADD COLUMN ") else {
        return Ok(false);
    };
    let table = after_alter[..add_pos].trim();
    let after_add = after_alter[add_pos + " ADD COLUMN ".len()..].trim_start();
    let column = after_add
        .split_whitespace()
        .next()
        .expect("static column name");
    Ok(connection
        .query_optional(
            "SELECT 1 FROM information_schema.columns WHERE table_schema = DATABASE() AND table_name = ?1 AND column_name = ?2 LIMIT 1",
            params![table, column],
        )?
        .is_some())
}
