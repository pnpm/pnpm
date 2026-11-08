use super::super::adapter::{Param, Query};
use pnpr_error::{RegistryError, Result};
use std::fmt::Write as _;

#[derive(Debug, Clone, Copy)]
pub(super) enum Dialect {
    Postgres,
    Mysql,
}

impl Dialect {
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
            _ => Err(RegistryError::InvalidConfig {
                reason: "unsupported metadata database URL".to_string(),
            }),
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

    pub(super) fn lock_schema(self) -> &'static str {
        match self {
            Self::Postgres => "SELECT 1 FROM pg_advisory_lock(70607832)",
            Self::Mysql => "SELECT GET_LOCK(CONCAT('pnpr_schema:', MD5(DATABASE())), 30)",
        }
    }

    pub(super) fn unlock_schema(self) -> &'static str {
        match self {
            Self::Postgres => "SELECT pg_advisory_unlock(70607832)",
            Self::Mysql => "SELECT RELEASE_LOCK(CONCAT('pnpr_schema:', MD5(DATABASE())))",
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

    pub(super) fn tables(self) -> String {
        match self {
            Self::Postgres => super::super::schema::TABLES
                .replace(" STRICT, WITHOUT ROWID", "")
                .replace(" STRICT", "")
                .replace(" BLOB", " BYTEA")
                .replace(" INTEGER", " BIGINT"),
            Self::Mysql => include_str!("mysql.sql").to_string(),
        }
    }

    fn functions(self, sql: &str) -> String {
        let mut sql = sql.to_string();
        if matches!(self, Self::Mysql) {
            sql = sql.replace(
                "SELECT value FROM json_each(",
                "SELECT JSON_UNQUOTE(value) FROM json_each(",
            );
        }
        sql = self.json_arrays(&sql);
        for index in 0..=1 {
            let source = format!("json_extract(value, '$[{index}]')");
            let target = match self {
                Self::Postgres => format!("CAST(value AS JSON)->>{index}"),
                Self::Mysql => format!("JSON_UNQUOTE(JSON_EXTRACT(value, '$[{index}]'))"),
            };
            sql = sql.replace(&source, &target);
        }
        sql
    }

    fn json_arrays(self, sql: &str) -> String {
        let mut rendered = String::new();
        let mut remaining = sql;
        while let Some((before, after)) = remaining.split_once("json_each(") {
            let Some((argument, after)) = after.split_once(')') else { break };
            rendered.push_str(before);
            let table = match self {
                Self::Postgres => {
                    format!("json_array_elements_text(CAST({argument} AS JSON)) AS elements(value)")
                }
                Self::Mysql => format!(
                    "JSON_TABLE({argument}, '$[*]' COLUMNS (value JSON PATH '$')) AS elements",
                ),
            };
            rendered.push_str(&table);
            remaining = after;
        }
        rendered.push_str(remaining);
        rendered
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
        let sql = self.functions(sql);
        let sql = match self {
            Self::Postgres => sql,
            Self::Mysql => mysql_upsert(&sql)?,
        };
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

fn mysql_upsert(sql: &str) -> Result<String> {
    let Some((insert, conflict)) = sql.split_once("ON CONFLICT") else {
        return Ok(sql.to_string());
    };
    if conflict.trim() == "DO NOTHING" {
        return ignore_duplicate(insert);
    }
    let (_, update) = conflict
        .split_once("DO UPDATE")
        .ok_or_else(|| RegistryError::Internal {
            reason: "unsupported metadata upsert".to_string(),
        })?;
    let update = update
        .trim()
        .strip_prefix("SET")
        .unwrap_or(update)
        .trim();
    let update = conditional_assignments(update);
    let rendered = mysql_inserted_values(&update);
    Ok(format!("{insert} ON DUPLICATE KEY UPDATE {rendered}"))
}

fn ignore_duplicate(insert: &str) -> Result<String> {
    let column = insert
        .split_once('(')
        .and_then(|(_, columns)| columns.split(',').next())
        .ok_or_else(|| RegistryError::Internal {
            reason: "metadata insert has no columns".to_string(),
        })?;
    let table = insert
        .split_whitespace()
        .nth(2)
        .ok_or_else(|| RegistryError::Internal {
            reason: "metadata insert has no table".to_string(),
        })?;
    Ok(format!("{insert} ON DUPLICATE KEY UPDATE {column} = {table}.{column}"))
}

fn conditional_assignments(update: &str) -> String {
    let Some((assignments, condition)) = update.split_once("WHERE") else {
        return update.to_string();
    };
    assignments
        .split(',')
        .map(|assignment| {
            let (column, value) = assignment.split_once('=').expect("static SQL assignment");
            format!("{column} = IF({condition}, {value}, {column})")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn mysql_inserted_values(update: &str) -> String {
    let mut rendered = String::new();
    let mut remaining = update;
    while let Some((before, after)) = remaining.split_once("excluded.") {
        rendered.push_str(before);
        let end = after
            .find(|character: char| !character.is_ascii_alphanumeric() && character != '_')
            .unwrap_or(after.len());
        let _ = write!(rendered, "VALUES({})", &after[..end]);
        remaining = &after[end..];
    }
    rendered.push_str(remaining);
    rendered
}

#[cfg(test)]
mod tests;
