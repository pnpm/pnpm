use crate::hrana::proto::Stmt;
use crate::hrana::stream::HranaStream;
use crate::hrana::{HttpSend, Result};
use crate::parser::StmtKind;
use crate::TransactionBehavior;
use libsql_hrana::proto::{ExecuteStreamReq, StreamRequest};

#[derive(Debug, Clone)]
pub(crate) struct HttpTransaction<T>
where
    T: HttpSend,
{
    stream: HranaStream<T>,
}

impl<T> HttpTransaction<T>
where
    T: HttpSend,
{
    pub fn stream(&self) -> &HranaStream<T> {
        &self.stream
    }

    pub async fn open(stream: HranaStream<T>, tx_behavior: TransactionBehavior) -> Result<Self> {
        let begin_stmt = match tx_behavior {
            TransactionBehavior::Deferred => "BEGIN DEFERRED",
            TransactionBehavior::Immediate => "BEGIN IMMEDIATE",
            TransactionBehavior::Exclusive => "BEGIN EXCLUSIVE",
            TransactionBehavior::ReadOnly => "BEGIN READONLY",
        };
        stream
            .execute_inner(Stmt::new(begin_stmt, false), false)
            .await?;
        Ok(HttpTransaction { stream })
    }

    pub async fn commit(&mut self) -> Result<()> {
        let stmt = Stmt::new("COMMIT", false);
        self.stream
            .finalize(StreamRequest::Execute(ExecuteStreamReq { stmt }))
            .await?;
        Ok(())
    }

    pub async fn rollback(&mut self) -> Result<()> {
        let stmt = Stmt::new("ROLLBACK", false);
        self.stream
            .finalize(StreamRequest::Execute(ExecuteStreamReq { stmt }))
            .await?;
        Ok(())
    }
}

/// Counts number of transaction begin statements and transaction commits/rollback
/// in order to determine if current statement execution will end within transaction
/// scope, will start a new transaction or end existing one.
#[repr(transparent)]
#[derive(Default)]
pub(crate) struct TxScopeCounter {
    scope: i32,
}

impl TxScopeCounter {
    pub(crate) fn count(&mut self, stmt_kind: StmtKind) {
        match stmt_kind {
            StmtKind::TxnBegin | StmtKind::TxnBeginReadOnly => self.scope += 1,
            StmtKind::TxnEnd => self.scope -= 1,
            _ => {}
        }
    }

    /// Check if within current scope we will eventually begin new transaction.
    pub(crate) fn begin_tx(&self) -> bool {
        self.scope > 0
    }

    /// Check if within current scope we will eventually close existing transaction.
    pub(crate) fn end_tx(&self) -> bool {
        self.scope < 0
    }
}
