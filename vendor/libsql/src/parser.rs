#![allow(dead_code)]

use crate::{Error, Result};
use fallible_iterator::FallibleIterator;
use sqlite3_parser::ast::{Cmd, PragmaBody, QualifiedName, Stmt, TransactionType};
use sqlite3_parser::lexer::sql::{Parser, ParserError};

/// A group of statements to be executed together.
#[derive(Debug, Clone)]
pub struct Statement {
    pub stmt: String,
    pub kind: StmtKind,
}

impl Default for Statement {
    fn default() -> Self {
        Self::empty()
    }
}

/// Classify statement in categories of interest.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum StmtKind {
    /// The begining of a transaction
    TxnBegin,
    TxnBeginReadOnly,
    /// The end of a transaction
    TxnEnd,
    Read,
    Write,
    Savepoint,
    Release,
    Attach,
    Detach,
    Other,
}

fn is_temp(name: &QualifiedName) -> bool {
    name.db_name.as_ref().map(|n| n.0.as_str()) == Some("TEMP")
}

fn is_reserved_tbl(name: &QualifiedName) -> bool {
    let n = name.name.0.to_lowercase();
    n == "_litestream_seq" || n == "_litestream_lock" || n == "libsql_wasm_func_table"
}

fn write_if_not_reserved(name: &QualifiedName) -> Option<StmtKind> {
    (!is_reserved_tbl(name)).then_some(StmtKind::Write)
}

impl StmtKind {
    fn kind(cmd: &Cmd) -> Option<Self> {
        match cmd {
            Cmd::Explain(Stmt::Pragma(name, body)) => Self::pragma_kind(name, body.as_ref()),
            Cmd::Explain(_) => Some(Self::Other),
            Cmd::ExplainQueryPlan(_) => Some(Self::Other),
            Cmd::Stmt(Stmt::Begin(Some(TransactionType::ReadOnly), _)) => {
                Some(Self::TxnBeginReadOnly)
            }
            Cmd::Stmt(Stmt::Begin { .. }) => Some(Self::TxnBegin),
            Cmd::Stmt(
                Stmt::Commit { .. }
                | Stmt::Rollback {
                    savepoint_name: None,
                    ..
                },
            ) => Some(Self::TxnEnd),
            Cmd::Stmt(
                Stmt::CreateVirtualTable { tbl_name, .. }
                | Stmt::CreateTable {
                    tbl_name,
                    temporary: false,
                    ..
                },
            ) if !is_temp(tbl_name) => Some(Self::Write),
            Cmd::Stmt(
                Stmt::Insert {
                    with: _,
                    or_conflict: _,
                    tbl_name,
                    ..
                }
                | Stmt::Update {
                    with: _,
                    or_conflict: _,
                    tbl_name,
                    ..
                },
            ) => write_if_not_reserved(tbl_name),

            Cmd::Stmt(Stmt::Delete {
                with: _, tbl_name, ..
            }) => write_if_not_reserved(tbl_name),
            Cmd::Stmt(Stmt::DropTable {
                if_exists: _,
                tbl_name,
            }) => write_if_not_reserved(tbl_name),
            Cmd::Stmt(Stmt::AlterTable(tbl_name, _)) => write_if_not_reserved(tbl_name),
            Cmd::Stmt(
                Stmt::DropIndex { .. }
                | Stmt::DropTrigger { .. }
                | Stmt::CreateTrigger {
                    temporary: false, ..
                }
                | Stmt::CreateIndex { .. },
            ) => Some(Self::Write),
            Cmd::Stmt(Stmt::Select { .. }) => Some(Self::Read),
            Cmd::Stmt(Stmt::Pragma(name, body)) => Self::pragma_kind(name, body.as_ref()),
            // Creating regular views is OK, temporary views are bound to a connection
            // and thus disallowed in sqld.
            Cmd::Stmt(Stmt::CreateView {
                temporary: false, ..
            }) => Some(Self::Write),
            Cmd::Stmt(Stmt::DropView { .. }) => Some(Self::Write),
            Cmd::Stmt(Stmt::Savepoint(_)) => Some(Self::Savepoint),
            Cmd::Stmt(Stmt::Release(_))
            | Cmd::Stmt(Stmt::Rollback {
                savepoint_name: Some(_),
                ..
            }) => Some(Self::Release),
            Cmd::Stmt(Stmt::Attach { .. }) => Some(Self::Attach),
            Cmd::Stmt(Stmt::Detach(_)) => Some(Self::Detach),
            Cmd::Stmt(Stmt::Reindex { .. }) => Some(Self::Write),
            _ => None,
        }
    }

    fn pragma_kind(name: &QualifiedName, body: Option<&PragmaBody>) -> Option<Self> {
        let name = name.name.0.as_str();
        match name {
            // always ok to be served by primary or replicas - pure readonly pragmas
            "table_list" | "index_list" | "table_info" | "table_xinfo" | "index_info" | "index_xinfo"
            | "pragma_list" | "compile_options" | "database_list" | "function_list"
            | "module_list" => Some(Self::Read),
            // special case for `encoding` - it's effectively readonly for connections
            // that already created a database, which is always the case for sqld
            "encoding" => Some(Self::Read),
            // always ok to be served by primary
            "defer_foreign_keys" | "foreign_keys" | "foreign_key_list" | "foreign_key_check" | "collation_list"
            | "data_version" | "freelist_count" | "integrity_check" | "legacy_file_format"
            | "page_count" | "quick_check" | "stats" | "user_version" => Some(Self::Write),
            // ok to be served by primary without args
            "analysis_limit"
            | "application_id"
            | "auto_vacuum"
            | "automatic_index"
            | "busy_timeout"
            | "cache_size"
            | "cache_spill"
            | "cell_size_check"
            | "checkpoint_fullfsync"
            | "fullfsync"
            | "hard_heap_limit"
            | "journal_mode"
            | "journal_size_limit"
            | "legacy_alter_table"
            | "locking_mode"
            | "max_page_count"
            | "mmap_size"
            | "page_size"
            | "query_only"
            | "read_uncommitted"
            | "recursive_triggers"
            | "reverse_unordered_selects"
            | "schema_version"
            | "secure_delete"
            | "soft_heap_limit"
            | "synchronous"
            | "temp_store"
            | "threads"
            | "trusted_schema"
            | "wal_autocheckpoint" => {
                match body {
                    Some(_) => None,
                    None => Some(Self::Write),
                }
            }
            // changes the state of the connection, and can't be allowed rn:
            "case_sensitive_like" | "ignore_check_constraints" | "incremental_vacuum"
                // TODO: check if optimize can be safely performed
                | "optimize"
                | "parser_trace"
                | "shrink_memory"
                | "wal_checkpoint" => None,
            _ => {
                tracing::debug!("Unknown pragma: {name}");
                None
            },
        }
    }
}

impl Statement {
    pub fn empty() -> Self {
        Self {
            stmt: String::new(),
            // empty statement is arbitrarely made of the read kind so it is not send to a writer
            kind: StmtKind::Read,
        }
    }

    pub fn parse(s: &str) -> impl Iterator<Item = Result<Self>> + '_ {
        fn parse_inner(
            original: &str,
            stmt_count: u64,
            has_more_stmts: bool,
            c: Cmd,
        ) -> Result<Statement> {
            let kind = StmtKind::kind(&c).ok_or_else(|| Error::Sqlite3UnsupportedStatement)?;

            if stmt_count == 1 && !has_more_stmts {
                // XXX: Temporary workaround for integration with Atlas
                if let Cmd::Stmt(Stmt::CreateTable { .. }) = &c {
                    return Ok(Statement {
                        stmt: original.to_string(),
                        kind,
                    });
                }
            }

            Ok(Statement {
                stmt: c.to_string(),
                kind,
            })
        }
        // The parser needs to be boxed because it's large, and you don't want it on the stack.
        // There's upstream work to make it smaller, but in the meantime the parser should remain
        // on the heap:
        // - https://github.com/gwenn/lemon-rs/issues/8
        // - https://github.com/gwenn/lemon-rs/pull/19
        let mut parser = Box::new(Parser::new(s.as_bytes()).peekable());
        let mut stmt_count = 0;
        std::iter::from_fn(move || {
            stmt_count += 1;
            match parser.next() {
                Ok(Some(cmd)) => Some(parse_inner(
                    s,
                    stmt_count,
                    parser.peek().map_or(true, |o| o.is_some()),
                    cmd,
                )),
                Ok(None) => None,
                Err(sqlite3_parser::lexer::sql::Error::ParserError(
                    ParserError::SyntaxError {
                        token_type: _,
                        found: Some(found),
                    },
                    Some((line, col)),
                )) => Some(Err(crate::Error::Sqlite3SyntaxError(line, col, found))),
                Err(e) => Some(Err(Error::Sqlite3ParserError(e.into()))),
            }
        })
    }

    pub fn is_read_only(&self) -> bool {
        matches!(
            self.kind,
            StmtKind::Read | StmtKind::TxnBeginReadOnly | StmtKind::TxnEnd
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_attach_same_db() {
        let input = "ATTACH test AS test;";
        let mut result = Statement::parse(input);

        let stmt = result.next().unwrap().unwrap();
        assert_eq!(stmt.kind, StmtKind::Attach);
    }

    #[test]
    fn test_attach_database() {
        let input = "ATTACH DATABASE test AS test;";
        let mut result = Statement::parse(input);

        let stmt = result.next().unwrap().unwrap();
        assert_eq!(stmt.kind, StmtKind::Attach);
    }

    #[test]
    fn test_attach_diff_db() {
        let input = "ATTACH \"random\" AS test;";
        let mut result = Statement::parse(input);

        let stmt = result.next().unwrap().unwrap();
        assert_eq!(stmt.kind, StmtKind::Attach);
    }

    #[test]
    fn test_attach_database_diff_db() {
        let input = "ATTACH DATABASE \"random\" AS test;";
        let mut result = Statement::parse(input);

        let stmt = result.next().unwrap().unwrap();
        assert_eq!(stmt.kind, StmtKind::Attach);
    }
}
