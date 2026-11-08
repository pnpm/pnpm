use super::super::{
    adapter::Connection,
    schema::{MARKER, SCHEMA_VERSION, statements},
};
use pnpr_error::Result;

pub(super) fn begin(connection: &dyn Connection) -> Result<()> {
    // MySQL commits DDL independently. The table comment survives even a lost CREATE response.
    connection.execute(
        &format!(
            "CREATE TABLE {MARKER} (version TEXT NOT NULL) ENGINE=InnoDB COMMENT='pnpr-init:{SCHEMA_VERSION}'",
        ),
        params![],
    )
}

pub(super) fn recover(connection: &dyn Connection) -> Result<bool> {
    let mut row = connection.query_one(
        "SELECT table_comment FROM information_schema.tables WHERE table_schema = DATABASE() AND table_name = ?1",
        params![MARKER],
    )?;
    let comment = row.get::<String>(0)?;
    if comment != format!("pnpr-init:{SCHEMA_VERSION}")
        || !connection
            .query(&format!("SELECT * FROM {MARKER}"), params![])?
            .is_empty()
    {
        return Ok(false);
    }
    finish(connection)?;
    Ok(true)
}

pub(super) fn finish(connection: &dyn Connection) -> Result<()> {
    for statement in statements(&connection.schema_tables()) {
        if !index_exists(connection, &statement)? {
            connection.execute(&statement, params![])?;
        }
    }
    connection.execute(
        &format!("INSERT INTO {MARKER} (version) VALUES (?1)"),
        params![SCHEMA_VERSION],
    )
}

fn index_exists(connection: &dyn Connection, statement: &str) -> Result<bool> {
    if !statement.starts_with("CREATE INDEX ") && !statement.starts_with("CREATE UNIQUE INDEX ") {
        return Ok(false);
    }
    let (_, index) = statement.split_once("INDEX ").expect("static index definition");
    let (name, table) = index.split_once(" ON ").expect("static index target");
    let table = table
        .split_whitespace()
        .next()
        .expect("static index table");
    Ok(connection.query_optional(
        "SELECT 1 FROM information_schema.statistics WHERE table_schema = DATABASE() AND table_name = ?1 AND index_name = ?2 LIMIT 1",
        params![table, name.trim()],
    )?.is_some())
}

#[cfg(all(test, feature = "backend-mysql"))]
mod tests;
