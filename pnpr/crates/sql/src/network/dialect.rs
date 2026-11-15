use super::super::{
    adapter::{Param, Query},
    dialect::Dialect,
};
use pnpr_error::{RegistryError, Result};
use std::{fmt::Write as _, time::Duration};

/// The second key of a `PostgreSQL` advisory lock, so pnpr instances on
/// different schemas of one database do not wait on each other's locks.
macro_rules! schema_key {
    () => {
        "hashtext(COALESCE(current_schema(), ''))"
    };
}

#[derive(Debug, Clone, Copy)]
pub(super) enum NetworkDialect {
    Postgres,
    Mysql,
}

impl NetworkDialect {
    pub(super) const fn dialect(self) -> Dialect {
        match self {
            Self::Postgres => Dialect::Postgres,
            Self::Mysql => Dialect::Mysql,
        }
    }

    pub(super) fn from_url(url: &str) -> Result<Self> {
        match url
            .split(':')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "postgres" | "postgresql" => Ok(Self::Postgres),
            "mysql" => Ok(Self::Mysql),
            _ => {
                Err(RegistryError::InvalidConfig { reason: "unsupported database URL".to_string() })
            }
        }
    }

    pub(super) fn retryable(error: &dyn sqlx::error::DatabaseError) -> bool {
        if matches!(error.code().as_deref(), Some("40001" | "40P01" | "23505")) {
            return true;
        }
        #[cfg(feature = "backend-mysql")]
        if let Some(error) = error.try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>() {
            return matches!(error.number(), 1213 | 1205 | 1615);
        }
        false
    }

    pub(super) fn lock_schema(self, timeout: Duration) -> String {
        match self {
            Self::Postgres => {
                concat!("SELECT 1 FROM pg_advisory_lock(70607832, ", schema_key!(), ")").to_string()
            }
            Self::Mysql => format!(
                "SELECT GET_LOCK(CONCAT('pnpr_schema:', MD5(DATABASE())), {})",
                timeout.as_secs_f64(),
            ),
        }
    }

    pub(super) fn unlock_schema(self) -> &'static str {
        match self {
            Self::Postgres => concat!("SELECT pg_advisory_unlock(70607832, ", schema_key!(), ")"),
            Self::Mysql => "SELECT RELEASE_LOCK(CONCAT('pnpr_schema:', MD5(DATABASE())))",
        }
    }

    /// The statement that takes the database's write lock, a session lock
    /// that returns `1` once held. Writers queue on it instead of failing
    /// each other's serializable transactions. It is taken before `BEGIN`, so
    /// the transaction's snapshot holds every commit made under it.
    pub(super) fn lock_writes(self, timeout: Duration) -> String {
        match self {
            Self::Postgres => {
                concat!("SELECT 1 FROM pg_advisory_lock(70607833, ", schema_key!(), ")").to_string()
            }
            Self::Mysql => format!(
                "SELECT GET_LOCK(CONCAT('pnpr_write:', MD5(DATABASE())), {})",
                timeout.as_secs_f64(),
            ),
        }
    }

    pub(super) fn unlock_writes(self) -> &'static str {
        match self {
            Self::Postgres => concat!("SELECT pg_advisory_unlock(70607833, ", schema_key!(), ")"),
            Self::Mysql => "DO RELEASE_LOCK(CONCAT('pnpr_write:', MD5(DATABASE())))",
        }
    }

    /// `COMMIT`, then [`Self::unlock_writes`] in the same round trip.
    pub(super) fn commit_write(self) -> &'static str {
        match self {
            Self::Postgres => {
                concat!("COMMIT; SELECT pg_advisory_unlock(70607833, ", schema_key!(), ")")
            }
            Self::Mysql => "COMMIT; DO RELEASE_LOCK(CONCAT('pnpr_write:', MD5(DATABASE())))",
        }
    }

    /// `ROLLBACK`, then [`Self::unlock_writes`] in the same round trip.
    pub(super) fn rollback_write(self) -> &'static str {
        match self {
            Self::Postgres => {
                concat!("ROLLBACK; SELECT pg_advisory_unlock(70607833, ", schema_key!(), ")")
            }
            Self::Mysql => "ROLLBACK; DO RELEASE_LOCK(CONCAT('pnpr_write:', MD5(DATABASE())))",
        }
    }

    pub(super) fn table_names(self) -> &'static str {
        match self {
            Self::Postgres => {
                "SELECT table_name FROM information_schema.tables WHERE table_schema = current_schema()"
            }
            Self::Mysql => {
                "SELECT table_name FROM information_schema.tables WHERE table_schema = DATABASE()"
            }
        }
    }

    pub(super) fn begin_write(self) -> &'static str {
        match self {
            Self::Postgres => "BEGIN ISOLATION LEVEL SERIALIZABLE",
            Self::Mysql => "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE; START TRANSACTION",
        }
    }

    pub(super) fn begin_read(self) -> &'static str {
        match self {
            Self::Postgres => "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY",
            Self::Mysql => {
                "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ; START TRANSACTION READ ONLY"
            }
        }
    }

    /// Syntax only the `SQLite` form of a [`DialectSql`](crate::DialectSql)
    /// holds, refused in debug builds so a statement that skipped it fails.
    fn sqlite_only(self) -> &'static [&'static str] {
        match self {
            Self::Postgres => &["json_each(", "json_extract("],
            Self::Mysql => &["json_each(", "json_extract(", "ON CONFLICT"],
        }
    }

    pub(super) fn statement<'value>(
        self,
        sql: &str,
        params: &[Param<'value>],
    ) -> Result<(String, Vec<Param<'value>>)> {
        let mut ordered = Vec::new();
        let sql = self.statement_into(sql, params, &mut ordered)?;
        Ok((sql, ordered))
    }

    pub(super) fn batch<'value>(
        self,
        queries: &[Query<'value>],
    ) -> Result<(String, Vec<Param<'value>>)> {
        let mut ordered = Vec::new();
        let mut statements = Vec::with_capacity(queries.len());
        let integer = match self {
            Self::Postgres => "BIGINT",
            Self::Mysql => "SIGNED",
        };
        for (index, query) in queries.iter().enumerate() {
            let sql = self.statement_into(query.sql, &query.params, &mut ordered)?;
            statements.push(format!(
                "SELECT CAST({index} AS {integer}) AS batch_index, batch.* FROM ({sql}) AS batch",
            ));
        }
        Ok((statements.join(" UNION ALL "), ordered))
    }

    fn statement_into<'value>(
        self,
        sql: &str,
        params: &[Param<'value>],
        ordered: &mut Vec<Param<'value>>,
    ) -> Result<String> {
        if cfg!(debug_assertions)
            && let Some(syntax) = self
                .sqlite_only()
                .iter()
                .find(|syntax| sql.contains(**syntax))
        {
            panic!("statement with SQLite syntax `{syntax}` reached {self:?}: {sql}");
        }
        let mut rendered = String::with_capacity(sql.len());
        let mut chars = sql.chars().peekable();
        let mut quoted = false;
        while let Some(character) = chars.next() {
            if character == '\'' {
                quoted = !quoted;
            }
            if character != '?' || quoted {
                rendered.push(character);
                continue;
            }
            ordered.push(next_parameter(&mut chars, params)?);
            match self {
                Self::Postgres => {
                    let _ = write!(rendered, "${}", ordered.len());
                }
                Self::Mysql => rendered.push('?'),
            }
        }
        Ok(rendered)
    }
}

fn next_parameter<'value>(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    params: &[Param<'value>],
) -> Result<Param<'value>> {
    let mut digits = String::new();
    while chars.peek().is_some_and(char::is_ascii_digit) {
        digits.push(chars.next().unwrap());
    }
    let index = digits
        .parse::<usize>()
        .ok()
        .and_then(|index| index.checked_sub(1));
    index
        .and_then(|index| params.get(index))
        .copied()
        .ok_or_else(|| RegistryError::Internal {
            reason: "metadata query has an invalid parameter index".to_string(),
        })
}

#[cfg(test)]
mod tests;
