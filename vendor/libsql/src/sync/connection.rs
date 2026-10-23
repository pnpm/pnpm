use crate::{
    connection::Conn,
    hrana::{connection::HttpConnection, hyper::HttpSender},
    local::{self, impls::LibsqlStmt},
    params::Params,
    parser,
    replication::connection::State,
    sync::SyncContext,
    BatchRows, Error, Result, Statement, Transaction, TransactionBehavior,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use super::{statement::SyncedStatement, transaction::SyncedTx};

#[derive(Clone)]
pub struct SyncedConnection {
    pub remote: HttpConnection<HttpSender>,
    pub local: local::Connection,
    pub read_your_writes: bool,
    pub context: Arc<Mutex<SyncContext>>,
    pub state: Arc<Mutex<State>>,
}

impl SyncedConnection {
    async fn should_execute_local(&self, sql: &str) -> Result<bool> {
        let stmts = crate::parser::Statement::parse(sql)
            .collect::<Result<Vec<_>>>()
            .or_else(|err| match err {
                Error::Sqlite3UnsupportedStatement => Ok(vec![]),
                err => Err(err),
            })?;

        let mut state = self.state.lock().await;

        if !self.remote.is_autocommit() {
            *state = State::Txn;
        }

        {
            let predicted_end_state = {
                let mut state = state.clone();

                stmts.iter().for_each(|parser::Statement { kind, .. }| {
                    state = state.step(*kind);
                });

                state
            };

            let should_execute_local = match (*state, predicted_end_state) {
                (State::Init, State::Init) => stmts.iter().all(parser::Statement::is_read_only),

                (State::Init, State::TxnReadOnly) | (State::TxnReadOnly, State::TxnReadOnly) => {
                    let is_read_only = stmts.iter().all(parser::Statement::is_read_only);

                    if !is_read_only {
                        return Err(Error::Misuse(
                            "Invalid write in a readonly transaction".into(),
                        ));
                    }

                    *state = State::TxnReadOnly;
                    true
                }

                (State::TxnReadOnly, State::Init) => {
                    let is_read_only = stmts.iter().all(parser::Statement::is_read_only);

                    if !is_read_only {
                        return Err(Error::Misuse(
                            "Invalid write in a readonly transaction".into(),
                        ));
                    }

                    *state = State::Init;
                    true
                }

                (init, State::Invalid) => {
                    let err = Err(Error::InvalidParserState(format!("{:?}", init)));

                    // Reset state always back to init so the user can start over
                    *state = State::Init;

                    return err;
                }
                _ => {
                    *state = predicted_end_state;
                    false
                }
            };

            Ok(should_execute_local)
        }
    }
}

#[async_trait::async_trait]
impl Conn for SyncedConnection {
    async fn execute(&self, sql: &str, params: Params) -> Result<u64> {
        let stmt = self.prepare(sql).await?;
        stmt.execute(params).await.map(|v| v as u64)
    }

    async fn execute_batch(&self, sql: &str) -> Result<BatchRows> {
        if self.should_execute_local(sql).await? {
            self.local.execute_batch(sql)
        } else {
            let result = self.remote.execute_batch(sql).await;
            if self.read_your_writes {
                let mut context = self.context.lock().await;
                crate::sync::try_pull(&mut context, &self.local).await?;
            }
            result
        }
    }

    async fn execute_transactional_batch(&self, sql: &str) -> Result<BatchRows> {
        if self.should_execute_local(sql).await? {
            self.local.execute_transactional_batch(sql)?;
            Ok(BatchRows::empty())
        } else {
            let result = self.remote.execute_transactional_batch(sql).await;
            if self.read_your_writes {
                let mut context = self.context.lock().await;
                crate::sync::try_pull(&mut context, &self.local).await?;
            }
            result
        }
    }

    async fn prepare(&self, sql: &str) -> Result<Statement> {
        if self.should_execute_local(sql).await? {
            Ok(Statement {
                inner: Box::new(LibsqlStmt(self.local.prepare(sql)?)),
            })
        } else {
            let stmt = Statement {
                inner: Box::new(self.remote.prepare(sql).await?),
            };

            Ok(Statement {
                inner: Box::new(SyncedStatement {
                    conn: self.local.clone(),
                    inner: stmt,
                    context: self.context.clone(),
                    read_your_writes: self.read_your_writes,
                }),
            })
        }
    }

    async fn transaction(&self, tx_behavior: TransactionBehavior) -> Result<Transaction> {
        let tx = SyncedTx::begin(self.clone(), tx_behavior).await?;

        Ok(Transaction {
            inner: Box::new(tx),
            conn: crate::Connection {
                conn: Arc::new(self.clone()),
            },
            close: None,
        })
    }

    fn interrupt(&self) -> Result<()> {
        Ok(())
    }

    fn busy_timeout(&self, timeout: Duration) -> Result<()> {
        self.remote.busy_timeout(timeout)
    }

    fn is_autocommit(&self) -> bool {
        self.remote.is_autocommit()
    }

    fn changes(&self) -> u64 {
        self.remote.changes()
    }

    fn total_changes(&self) -> u64 {
        self.remote.total_changes()
    }

    fn last_insert_rowid(&self) -> i64 {
        self.remote.last_insert_rowid()
    }

    async fn reset(&self) {}

    fn set_reserved_bytes(&self, reserved_bytes: i32) -> Result<()> {
        self.local.set_reserved_bytes(reserved_bytes)
    }

    fn get_reserved_bytes(&self) -> Result<i32> {
        self.local.get_reserved_bytes()
    }
}
