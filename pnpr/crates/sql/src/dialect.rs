//! The SQL dialects the shared layer runs on.

/// The SQL dialect a [`Connection`](crate::adapter::Connection) runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// `SQLite` and `libSQL`.
    Sqlite,
    Postgres,
    Mysql,
}

/// A statement written once per [`Dialect`], with `?N` placeholders.
///
/// Statements the dialects spell differently, such as upserts and JSON
/// array parameters, are written out for each one. A connection translates
/// only the placeholders of the statement it is given.
#[derive(Debug, Clone, Copy)]
pub struct DialectSql {
    pub sqlite: &'static str,
    pub postgres: &'static str,
    pub mysql: &'static str,
}

impl DialectSql {
    /// A statement `SQLite` and `PostgreSQL` share, with its `MySQL` form.
    #[must_use]
    pub const fn with_mysql(shared: &'static str, mysql: &'static str) -> Self {
        Self { sqlite: shared, postgres: shared, mysql }
    }

    /// The statement for `dialect`.
    #[must_use]
    pub const fn get(&self, dialect: Dialect) -> &'static str {
        match dialect {
            Dialect::Sqlite => self.sqlite,
            Dialect::Postgres => self.postgres,
            Dialect::Mysql => self.mysql,
        }
    }
}
