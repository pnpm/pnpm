//! The metadata database's tables.
//!
//! A package is keyed by `(catalog, ecosystem, name)`: the organization
//! namespace it is hosted in, its ecosystem, and its canonical name. Every key
//! column is `NOT NULL`, so no nullable column can weaken a uniqueness
//! constraint. `revision_key` is `ordinary` for a version outside the registry
//! revision protocol.

use super::{
    adapter::{Connection, Value},
    batch::WriteTransaction,
};
use pnpr_error::Result;
use std::fmt;

/// The version of the metadata layout in [`TABLES`], stored as text in the
/// [`MARKER`].
///
/// pnpr does not migrate the metadata before 1.0. Until then the version is
/// `0.x`: any change to the metadata layout, a table, column, constraint, or
/// index, bumps its minor part (`0.2`, `0.3`, ...). A database that holds
/// another version is refused, and the operator wipes pnpr's metadata tables
/// by hand so that pnpr creates them afresh. With 1.0 the schema moves to
/// numbered migrations.
pub(super) const SCHEMA_VERSION: &str = "0.2";

/// The table that marks a database as pnpr's metadata. Its one row is the
/// [`SCHEMA_VERSION`] of the layout that created the tables.
pub(super) const MARKER: &str = "schema_version";

/// The statement that creates the [`MARKER`] table.
pub(super) const MARKER_TABLE: &str = "CREATE TABLE schema_version (version TEXT NOT NULL) STRICT";

/// The tables and indexes of the metadata. A change to a statement here,
/// other than to its comments or whitespace, bumps [`SCHEMA_VERSION`].
pub(super) const TABLES: &str = include_str!("schema/tables.sql");

/// What a database holds where pnpr expects its metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Stored {
    /// The marker holds this schema version, as text.
    Version(String),
    /// The marker holds an integer schema version, as an older pnpr's marker
    /// does.
    Integer(i64),
    /// The marker holds anything else, or more or fewer than one row.
    Unreadable,
    /// Metadata tables without a marker.
    Unmarked,
    /// The marker names this pnpr's version, but these metadata tables are
    /// missing, as after a wipe that left the marker.
    Incomplete(Vec<String>),
}

impl fmt::Display for Stored {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Version(version) => write!(f, "schema version {version}"),
            Self::Integer(version) => write!(f, "the integer schema version {version}"),
            Self::Unreadable => f.write_str("an unreadable schema marker"),
            Self::Unmarked => f.write_str("metadata tables without a schema marker"),
            Self::Incomplete(missing) => write!(
                f,
                "schema version {SCHEMA_VERSION} without the metadata tables {}",
                missing.join(", "),
            ),
        }
    }
}

/// Create the schema in an empty database, and report what a database that
/// is not empty holds unless it is this pnpr's schema.
///
/// The database is inspected before anything is created, so a database that
/// is refused is left as it was. A database counts as empty when it has no
/// marker and none of the metadata tables; tables of anything else that
/// shares it, such as pnpr's auth tables, do not count.
pub(super) fn prepare(connection: &dyn Connection) -> Result<Option<Stored>> {
    let transaction = WriteTransaction::begin(connection)?;
    let present = transaction
        .query("SELECT name FROM sqlite_master WHERE type = 'table'", params![])?
        .into_iter()
        .map(|mut row| row.get::<String>(0))
        .collect::<Result<Vec<_>>>()?;
    // `SQLite` matches table names without regard to ASCII case.
    let has = |name: &str| {
        present
            .iter()
            .any(|table| table.eq_ignore_ascii_case(name))
    };
    if has(MARKER) {
        return Ok(match stored(&*transaction)? {
            Stored::Version(stored) if stored == SCHEMA_VERSION => incomplete(has),
            stored => Some(stored),
        });
    }
    if table_names(TABLES).iter().any(|table| has(table)) {
        return Ok(Some(Stored::Unmarked));
    }
    for statement in statements(TABLES) {
        transaction.execute(&statement, params![])?;
    }
    transaction.execute(MARKER_TABLE, params![])?;
    transaction.execute(
        &format!("INSERT INTO {MARKER} (version) VALUES (?1)"),
        params![SCHEMA_VERSION],
    )?;
    transaction.commit()?;
    Ok(None)
}

/// The metadata tables a database marked with this pnpr's version lacks, given
/// whether it `has` each table.
fn incomplete(has: impl Fn(&str) -> bool) -> Option<Stored> {
    let missing = table_names(TABLES)
        .into_iter()
        .filter(|table| !has(table))
        .collect::<Vec<_>>();
    (!missing.is_empty()).then_some(Stored::Incomplete(missing))
}

/// What the marker holds. It is read whole, so a marker of any layout is
/// read rather than failing on a column it lacks.
fn stored(connection: &dyn Connection) -> Result<Stored> {
    let mut rows = connection.query(&format!("SELECT * FROM {MARKER}"), params![])?;
    if rows.len() != 1 {
        return Ok(Stored::Unreadable);
    }
    Ok(match rows.remove(0).get::<Value>(0) {
        Ok(Value::Text(version)) => Stored::Version(version),
        Ok(Value::Integer(version)) => Stored::Integer(version),
        _ => Stored::Unreadable,
    })
}

/// The tables `script` creates, however its statements are spaced or cased.
pub(super) fn table_names(script: &str) -> Vec<String> {
    statements(script)
        .filter_map(|statement| {
            let statement = statement
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            // An ASCII case change keeps every byte offset.
            let upper = statement.to_ascii_uppercase();
            let mut start = "CREATE TABLE ".len();
            if !upper.starts_with("CREATE TABLE ") {
                return None;
            }
            if upper[start..].starts_with("IF NOT EXISTS ") {
                start += "IF NOT EXISTS ".len();
            }
            statement[start..]
                .split(|character: char| character.is_whitespace() || character == '(')
                .next()
                .map(str::to_string)
        })
        .collect()
}

/// The statements of `script`, without its comments. Each runs on its own:
/// a libSQL client re-renders the statements of a batch, and its rendering
/// drops the comma between `STRICT` and `WITHOUT ROWID`.
fn statements(script: &str) -> impl Iterator<Item = String> + '_ {
    let code: String = script
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    code.split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>()
        .into_iter()
}
