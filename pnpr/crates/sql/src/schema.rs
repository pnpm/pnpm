//! Numbered forward-only schema migrations for pnpr's metadata and auth tables.
//!
//! A package is keyed by `(catalog, ecosystem, name)`: the organization
//! namespace it is hosted in, its ecosystem, and its canonical name. Every key
//! column is `NOT NULL`, so no nullable column can weaken a uniqueness
//! constraint. `revision_key` is `ordinary` for a version outside the registry
//! revision protocol.
//!
//! [`STEPS`] lists the numbered migrations starting with step `1`, the 1.0
//! schema. [`prepare`] acquires the database write lock, inspects [`MARKER`]
//! (`schema_migrations`), refuses pre-1.0 databases without [`MARKER`] or
//! databases from a newer release, and runs each pending step in order.

use crate::adapter::{Adapter, Connection, Value, WriteTransaction};
use pnpr_error::{RegistryError, Result};
use std::{
    fmt,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// How many times schema migration retries a transient transaction conflict.
pub(crate) const MIGRATION_ATTEMPTS: u32 = 8;

/// The initial wait before retrying a transient migration conflict.
const FIRST_RETRY_DELAY: Duration = Duration::from_millis(10);

/// Backoff delay before retrying transient migration conflict `attempt`.
pub(crate) fn retry_delay(attempt: u32) -> Duration {
    FIRST_RETRY_DELAY * (1 << attempt.min(5))
}

/// The latest schema version this pnpr creates and reads (`STEPS.len()`).
pub const SCHEMA_VERSION: i64 = 3;

/// The table recording each applied migration step (`1..=SCHEMA_VERSION`).
pub const MARKER: &str = "schema_migrations";

/// The pre-1.0 metadata schema marker table, refused without migration.
pub const LEGACY_MARKER: &str = "schema_version";

/// The pre-1.0 auth migrations table, refused without migration.
pub const LEGACY_AUTH_MARKER: &str = "auth_schema_migrations";

/// The statement that creates the [`MARKER`] table on `SQLite` and `libSQL`.
pub const MARKER_TABLE: &str = "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER NOT NULL PRIMARY KEY, applied_at INTEGER NOT NULL DEFAULT 0) STRICT";

/// The tables and indexes created by step 1.
pub const TABLES: &str = include_str!("schema/tables.sql");

/// One forward-only migration step.
///
/// On databases where DDL commits implicitly (`MySQL`), a step can run again
/// after an interruption, because its version is recorded only after it
/// returns. Write each step so that a second run gives the same result: run
/// DDL through [`Connection::execute_ddl`] and make data fixes idempotent.
pub type Step = fn(&dyn Connection) -> Result<()>;

/// Every numbered migration step in order (`1..=SCHEMA_VERSION`).
pub const STEPS: &[Step] = &[step_1, step_2, step_3];

const _: () = assert!(
    SCHEMA_VERSION > 0 && STEPS.len() == SCHEMA_VERSION.cast_unsigned() as usize,
    "SCHEMA_VERSION must equal STEPS.len()",
);

/// Step 1: the 1.0 metadata and auth schema.
pub fn step_1(connection: &dyn Connection) -> Result<()> {
    let tables = connection.schema_tables().unwrap_or_else(|| TABLES.to_string());
    for statement in statements(&tables) {
        connection.execute_ddl(&statement)?;
    }
    Ok(())
}

fn step_2(connection: &dyn Connection) -> Result<()> {
    connection.execute_ddl("CREATE INDEX IF NOT EXISTS registry_records_by_generation ON registry_records (kind, generation)")
}

fn step_3(connection: &dyn Connection) -> Result<()> {
    let mut rows = connection.query("SELECT * FROM schema_migrations", params![])?;
    if rows
        .first_mut()
        .is_some_and(|row| row.len() == 1)
    {
        let column_type = if connection.schema_marker().is_some() { "BIGINT" } else { "INTEGER" };
        connection.execute_ddl(&format!(
            "ALTER TABLE schema_migrations ADD COLUMN applied_at {column_type} NOT NULL DEFAULT 0",
        ))?;
    }
    Ok(())
}

fn record_step(connection: &dyn Connection, version: i64) -> Result<()> {
    connection.execute("INSERT INTO schema_migrations (version) VALUES (?1)", params![version])?;
    let row = connection.query_one(
        "SELECT * FROM schema_migrations WHERE version = ?1",
        params![version],
    )?;
    if row.len() == 1 {
        return Ok(());
    }
    let applied_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let applied_at = i64::try_from(applied_at).unwrap_or(i64::MAX);
    connection.execute(
        "UPDATE schema_migrations SET applied_at = ?1 WHERE version = ?2",
        params![applied_at, version],
    )
}

/// The unsupported marker read from a running database, distinct from a failed probe.
pub fn check_current(connection: &dyn Connection) -> Result<Option<Stored>> {
    let rows = connection.query("SELECT version FROM schema_migrations", params![])?;
    let Some(version) = parse_contiguous_versions(rows) else {
        return Ok(Some(Stored::Unreadable));
    };
    Ok(match version.cmp(&SCHEMA_VERSION) {
        std::cmp::Ordering::Greater => Some(Stored::FutureVersion(version)),
        std::cmp::Ordering::Less => Some(Stored::Unreadable),
        std::cmp::Ordering::Equal => None,
    })
}

/// What a database holds when it cannot be migrated to [`SCHEMA_VERSION`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stored {
    /// [`MARKER`] holds a version newer than [`SCHEMA_VERSION`].
    FutureVersion(i64),
    /// The pre-1.0 [`LEGACY_MARKER`] table holds this text version.
    Pre1_0Version(String),
    /// The pre-1.0 [`LEGACY_MARKER`] table holds this integer version.
    Pre1_0Integer(i64),
    /// The pre-1.0 [`LEGACY_AUTH_MARKER`] table is present without [`MARKER`].
    Pre1_0Auth,
    /// [`MARKER`] holds anything else, or no valid rows.
    Unreadable,
    /// [`LEGACY_MARKER`] holds anything else, or no valid rows.
    UnreadableLegacy,
    /// Metadata or auth tables without [`MARKER`].
    Unmarked,
    /// [`MARKER`] is present, but these tables are missing, as after a partial
    /// wipe that left [`MARKER`].
    Incomplete(Vec<String>),
}

impl fmt::Display for Stored {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FutureVersion(version) => write!(f, "schema version {version}"),
            Self::Pre1_0Version(version) => write!(f, "pre-1.0 schema version {version}"),
            Self::Pre1_0Integer(version) => {
                write!(f, "the pre-1.0 integer schema version {version}")
            }
            Self::Pre1_0Auth => {
                write!(f, "pre-1.0 {LEGACY_AUTH_MARKER} tables without a {MARKER} table")
            }
            Self::Unreadable => f.write_str("an unreadable schema_migrations table"),
            Self::UnreadableLegacy => f.write_str("an unreadable schema_version table"),
            Self::Unmarked => {
                f.write_str("metadata or auth tables without a schema_migrations table")
            }
            Self::Incomplete(missing) => write!(
                f,
                "schema version {SCHEMA_VERSION} without the tables {}",
                missing.join(", "),
            ),
        }
    }
}

/// Migrate `connection` to [`SCHEMA_VERSION`], or return an error naming `name`
/// and what the database holds when it is incompatible.
pub fn ensure(connection: &dyn Connection, name: &str) -> Result<()> {
    if let Some(stored) = prepare(connection)? {
        return Err(refused_error(name, &stored));
    }
    Ok(())
}

/// Format the error returned when `name` holds an incompatible schema `stored`.
#[must_use]
pub fn refused_error(name: &str, stored: &Stored) -> RegistryError {
    let reason = match stored {
        Stored::FutureVersion(_) => format!(
            "the database {name} holds {stored}, which is newer than this pnpr (expects schema version {SCHEMA_VERSION}). Upgrade pnpr to read this database",
        ),
        Stored::Unreadable | Stored::Incomplete(_) => format!(
            "the database {name} holds {stored}, which is incompatible with this pnpr \
             (expects schema version {SCHEMA_VERSION}).",
        ),
        Stored::Pre1_0Version(_)
        | Stored::Pre1_0Integer(_)
        | Stored::Pre1_0Auth
        | Stored::UnreadableLegacy
        | Stored::Unmarked => format!(
            "the database {name} holds {stored}, which is incompatible with this pnpr \
             (expects schema version {SCHEMA_VERSION}). Pre-1.0 databases without {MARKER} \
             are not migrated; wipe pnpr's tables and its hosted store together, and pnpr \
             creates them afresh",
        ),
    };
    RegistryError::InvalidConfig { reason }
}

/// Apply all pending [`STEPS`] on `connection`, and report what a database
/// that cannot be migrated holds.
///
/// The database is inspected before anything is created, so a database that
/// is refused is left as it was.
pub fn prepare(connection: &dyn Connection) -> Result<Option<Stored>> {
    prepare_with_steps(connection, STEPS)
}

/// Apply `steps` (`1..=steps.len()`) through `adapter`, retrying transient
/// transaction conflicts reported by [`Adapter::is_retryable`].
pub fn prepare_adapter_with_steps(adapter: &dyn Adapter, steps: &[Step]) -> Result<Option<Stored>> {
    let mut attempt = 0;
    loop {
        let mut refused = None;
        let result = adapter.initialize(&mut |connection| {
            refused = prepare_with_steps(connection, steps)?;
            Ok(())
        });
        match result {
            Ok(()) => return Ok(refused),
            Err(error) if attempt + 1 < MIGRATION_ATTEMPTS && adapter.is_retryable(&error) => {
                std::thread::sleep(retry_delay(attempt));
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Apply `steps` (`1..=steps.len()`) on `connection` under the database write
/// lock.
///
/// On databases with transactional DDL (`SQLite`, `libSQL`, `PostgreSQL`), each
/// step and its [`MARKER`] row run in one write transaction. On `MySQL`, where
/// DDL commits implicitly, each step's version is recorded in [`MARKER`] only
/// after the step completes.
pub fn prepare_with_steps(connection: &dyn Connection, steps: &[Step]) -> Result<Option<Stored>> {
    connection.lock_schema()?;
    if connection.transactional_ddl() {
        prepare_transactional(connection, steps)
    } else {
        prepare_non_transactional(connection, steps)
    }
}

fn prepare_transactional(connection: &dyn Connection, steps: &[Step]) -> Result<Option<Stored>> {
    let target = i64::try_from(steps.len()).unwrap_or(i64::MAX);
    loop {
        let transaction = WriteTransaction::begin(connection)?;
        let current = match inspect(&*transaction, target)? {
            Inspection::Refused(stored) => return Ok(Some(stored)),
            Inspection::Current(version) => version,
        };
        if current == target {
            transaction.commit()?;
            return Ok(None);
        }
        if current == 0 {
            let marker = transaction.schema_marker().unwrap_or(MARKER_TABLE);
            transaction.execute(marker, params![])?;
        }
        let next = current + 1;
        let index = usize::try_from(current).expect("0 <= current < steps.len()");
        steps[index](&*transaction)?;
        record_step(&*transaction, next)?;
        transaction.commit()?;
        if next == target {
            return Ok(None);
        }
    }
}

fn prepare_non_transactional(
    connection: &dyn Connection,
    steps: &[Step],
) -> Result<Option<Stored>> {
    let target = i64::try_from(steps.len()).unwrap_or(i64::MAX);
    let current = match inspect(connection, target)? {
        Inspection::Refused(stored) => return Ok(Some(stored)),
        Inspection::Current(version) => version,
    };
    if current == target {
        return Ok(None);
    }
    if current == 0 {
        let marker = connection.schema_marker().unwrap_or(MARKER_TABLE);
        connection.execute(marker, params![])?;
    }
    let start = usize::try_from(current).expect("0 <= current < steps.len()");
    for (index, step) in steps.iter().enumerate().skip(start) {
        let version = i64::try_from(index + 1).expect("step index fits in i64");
        step(connection)?;
        record_step(connection, version)?;
    }
    Ok(None)
}

enum Inspection {
    Current(i64),
    Refused(Stored),
}

fn inspect(connection: &dyn Connection, target: i64) -> Result<Inspection> {
    let present = connection
        .schema_table_names()?
        .into_iter()
        .map(|mut row| row.get::<String>(0))
        .collect::<Result<Vec<_>>>()?;
    // `SQLite` matches table names without regard to ASCII case.
    let has = |name: &str| {
        present
            .iter()
            .any(|table| table.eq_ignore_ascii_case(name))
    };
    if !has(MARKER) {
        return inspect_without_marker(connection, has);
    }
    inspect_marked(connection, target, has)
}

fn inspect_without_marker(
    connection: &dyn Connection,
    has: impl Fn(&str) -> bool,
) -> Result<Inspection> {
    if has(LEGACY_MARKER) {
        return Ok(Inspection::Refused(stored_legacy(connection)?));
    }
    if has(LEGACY_AUTH_MARKER) {
        return Ok(Inspection::Refused(Stored::Pre1_0Auth));
    }
    if table_names(TABLES).iter().any(|table| has(table)) {
        return Ok(Inspection::Refused(Stored::Unmarked));
    }
    Ok(Inspection::Current(0))
}

fn inspect_marked(
    connection: &dyn Connection,
    target: i64,
    has: impl Fn(&str) -> bool,
) -> Result<Inspection> {
    let rows = connection.query(&format!("SELECT version FROM {MARKER}"), params![])?;
    if rows.is_empty() {
        if connection.is_recoverable_empty_migrations()? {
            return Ok(Inspection::Current(0));
        }
        return Ok(Inspection::Refused(Stored::Unreadable));
    }
    let Some(max_version) = parse_contiguous_versions(rows) else {
        return Ok(Inspection::Refused(Stored::Unreadable));
    };
    if max_version > target {
        return Ok(Inspection::Refused(Stored::FutureVersion(max_version)));
    }
    if let Some(stored) = incomplete(has) {
        return Ok(Inspection::Refused(stored));
    }
    Ok(Inspection::Current(max_version))
}

fn parse_contiguous_versions(rows: Vec<crate::adapter::Row>) -> Option<i64> {
    let mut versions = Vec::with_capacity(rows.len());
    for mut row in rows {
        if row.len() != 1 {
            return None;
        }
        let Ok(Value::Integer(version)) = row.get::<Value>(0) else {
            return None;
        };
        if version < 1 {
            return None;
        }
        versions.push(version);
    }
    versions.sort_unstable();
    let max_version = *versions.last()?;
    versions
        .iter()
        .copied()
        .eq(1..=max_version)
        .then_some(max_version)
}

fn stored_legacy(connection: &dyn Connection) -> Result<Stored> {
    let mut rows = connection.query(&format!("SELECT * FROM {LEGACY_MARKER}"), params![])?;
    if rows.len() != 1 || rows[0].len() != 1 {
        return Ok(Stored::UnreadableLegacy);
    }
    Ok(match rows.remove(0).get::<Value>(0) {
        Ok(Value::Text(version)) => Stored::Pre1_0Version(version),
        Ok(Value::Integer(version)) => Stored::Pre1_0Integer(version),
        _ => Stored::UnreadableLegacy,
    })
}

/// The tables a database marked with [`MARKER`] lacks, given whether it `has`
/// each table.
fn incomplete(has: impl Fn(&str) -> bool) -> Option<Stored> {
    let missing = table_names(TABLES)
        .into_iter()
        .filter(|table| !has(table))
        .collect::<Vec<_>>();
    (!missing.is_empty()).then_some(Stored::Incomplete(missing))
}

/// The tables `script` creates, however its statements are spaced or cased.
#[must_use]
pub fn table_names(script: &str) -> Vec<String> {
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
pub fn statements(script: &str) -> impl Iterator<Item = String> + '_ {
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

#[cfg(test)]
mod diagnostic_tests;
