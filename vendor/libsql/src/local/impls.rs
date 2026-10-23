use std::sync::Arc;
use std::time::Duration;
use std::{fmt, path::Path};

use crate::{
    connection::{AuthHook, BatchRows, Conn, UpdateHook},
    params::Params,
    rows::{ColumnsInner, RowInner, RowsInner},
    statement::Stmt,
    transaction::Tx,
    Column, Connection, Result, Row, Rows, Statement, Transaction, TransactionBehavior, Value,
    ValueType,
};

#[derive(Clone)]
pub(crate) struct LibsqlConnection {
    pub(crate) conn: super::Connection,
}

#[async_trait::async_trait]
impl Conn for LibsqlConnection {
    async fn execute(&self, sql: &str, params: Params) -> Result<u64> {
        self.conn.execute(sql, params)
    }

    async fn execute_batch(&self, sql: &str) -> Result<BatchRows> {
        self.conn.execute_batch(sql)
    }

    async fn execute_transactional_batch(&self, sql: &str) -> Result<BatchRows> {
        self.conn.execute_transactional_batch(sql)?;
        Ok(BatchRows::empty())
    }

    async fn prepare(&self, sql: &str) -> Result<Statement> {
        let sql = sql.to_string();

        let stmt = self.conn.prepare(sql)?;

        Ok(Statement {
            inner: Box::new(LibsqlStmt(stmt)),
        })
    }

    async fn transaction(&self, tx_behavior: TransactionBehavior) -> Result<Transaction> {
        let tx = crate::local::Transaction::begin(self.conn.clone(), tx_behavior)?;
        // TODO(lucio): Can we just use the conn passed to the transaction?
        Ok(Transaction {
            inner: Box::new(LibsqlTx(Some(tx))),
            conn: Connection {
                conn: Arc::new(self.clone()),
            },
            close: None,
        })
    }

    fn interrupt(&self) -> Result<()> {
        self.conn.interrupt()
    }

    fn busy_timeout(&self, timeout: Duration) -> Result<()> {
        self.conn.busy_timeout(timeout)
    }

    fn is_autocommit(&self) -> bool {
        self.conn.is_autocommit()
    }

    fn changes(&self) -> u64 {
        self.conn.changes()
    }

    fn total_changes(&self) -> u64 {
        self.conn.total_changes()
    }

    fn last_insert_rowid(&self) -> i64 {
        self.conn.last_insert_rowid()
    }

    async fn reset(&self) {}

    fn set_reserved_bytes(&self, reserved_bytes: i32) -> Result<()> {
        self.conn.set_reserved_bytes(reserved_bytes)
    }

    fn get_reserved_bytes(&self) -> Result<i32> {
        self.conn.get_reserved_bytes()
    }

    fn enable_load_extension(&self, onoff: bool) -> Result<()> {
        self.conn.enable_load_extension(onoff)
    }

    fn load_extension(&self, dylib_path: &Path, entry_point: Option<&str>) -> Result<()> {
        self.conn.load_extension(dylib_path, entry_point)
    }

    fn authorizer(&self, hook: Option<AuthHook>) -> Result<()> {
        self.conn.authorizer(hook)
    }

    fn add_update_hook(&self, cb: Box<UpdateHook>) -> Result<()> {
        Ok(self.conn.add_update_hook(cb))
    }
}

impl Drop for LibsqlConnection {
    fn drop(&mut self) {
        self.conn.disconnect()
    }
}

pub(crate) struct LibsqlStmt(pub crate::local::Statement);

#[async_trait::async_trait]
impl Stmt for LibsqlStmt {
    fn finalize(&mut self) {
        self.0.finalize();
    }

    async fn execute(&self, params: &Params) -> Result<usize> {
        let params = params.clone();
        let stmt = self.0.clone();

        stmt.execute(&params).map(|i| i as usize)
    }

    async fn query(&self, params: &Params) -> Result<Rows> {
        let params = params.clone();
        let stmt = self.0.clone();

        stmt.query(&params).map(LibsqlRows).map(Rows::new)
    }

    async fn run(&self, params: &Params) -> Result<()> {
        let params = params.clone();
        let stmt = self.0.clone();

        stmt.run(&params)
    }

    fn interrupt(&self) -> Result<()> {
        self.0.interrupt()
    }

    fn reset(&self) {
        self.0.reset();
    }

    fn parameter_count(&self) -> usize {
        self.0.parameter_count()
    }

    fn parameter_name(&self, idx: i32) -> Option<&str> {
        self.0.parameter_name(idx)
    }

    fn column_count(&self) -> usize {
        self.0.column_count()
    }

    fn columns(&self) -> Vec<Column> {
        self.0.columns()
    }
}

pub(super) struct LibsqlTx(pub(super) Option<crate::local::Transaction>);

#[async_trait::async_trait]
impl Tx for LibsqlTx {
    async fn commit(&mut self) -> Result<()> {
        let tx = self.0.take().expect("Tx already dropped");
        tx.commit()
    }

    async fn rollback(&mut self) -> Result<()> {
        let tx = self.0.take().expect("Tx already dropped");
        tx.rollback()
    }
}

pub(crate) struct LibsqlRows(pub(crate) crate::local::Rows);

#[async_trait::async_trait]
impl RowsInner for LibsqlRows {
    async fn next(&mut self) -> Result<Option<Row>> {
        let row = self.0.next()?.map(|r| Row {
            inner: Box::new(LibsqlRow(r)),
        });

        Ok(row)
    }
}

impl ColumnsInner for LibsqlRows {
    fn column_count(&self) -> i32 {
        self.0.column_count()
    }

    fn column_name(&self, idx: i32) -> Option<&str> {
        self.0.column_name(idx)
    }

    fn column_type(&self, idx: i32) -> Result<ValueType> {
        self.0.column_type(idx).map(ValueType::from)
    }
}

struct LibsqlRow(crate::local::Row);

impl RowInner for LibsqlRow {
    fn column_value(&self, idx: i32) -> Result<Value> {
        self.0.get_value(idx)
    }

    fn column_str(&self, idx: i32) -> Result<&str> {
        self.0.get::<&str>(idx)
    }
}

impl ColumnsInner for LibsqlRow {
    fn column_name(&self, idx: i32) -> Option<&str> {
        self.0.column_name(idx)
    }

    fn column_type(&self, idx: i32) -> Result<ValueType> {
        self.0.column_type(idx).map(ValueType::from)
    }

    fn column_count(&self) -> i32 {
        self.0.stmt.column_count() as i32
    }
}

impl fmt::Debug for LibsqlRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::result::Result<(), fmt::Error> {
        self.0.fmt(f)
    }
}
