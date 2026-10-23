use crate::{
    local::{self},
    params::Params,
    statement::Stmt,
    sync::SyncContext, Column, Result, Rows, Statement,
};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct SyncedStatement {
    pub conn: local::Connection,
    pub inner: Statement,
    pub context: Arc<Mutex<SyncContext>>,
    pub read_your_writes: bool,
}

#[async_trait::async_trait]
impl Stmt for SyncedStatement {
    fn finalize(&mut self) {
        self.inner.finalize()
    }

    async fn execute(&self, params: &Params) -> Result<usize> {
        let result = self.inner.execute(params).await;
        if self.read_your_writes {
            let mut context = self.context.lock().await;
            crate::sync::try_pull(&mut context, &self.conn).await?;
        }
        result
    }

    async fn query(&self, params: &Params) -> Result<Rows> {
        let result = self.inner.query(params).await;
        if self.read_your_writes {
            let mut context = self.context.lock().await;
            crate::sync::try_pull(&mut context, &self.conn).await?;
        }
        result
    }

    async fn run(&self, params: &Params) -> Result<()> {
        let result = self.inner.run(params).await;
        if self.read_your_writes {
            let mut context = self.context.lock().await;
            crate::sync::try_pull(&mut context, &self.conn).await?;
        }
        result
    }

    fn interrupt(&self) -> Result<()> {
        self.inner.interrupt()
    }

    fn reset(&self) {
        self.inner.reset()
    }

    fn parameter_count(&self) -> usize {
        self.inner.parameter_count()
    }

    fn parameter_name(&self, idx: i32) -> Option<&str> {
        self.inner.parameter_name(idx)
    }

    fn column_count(&self) -> usize {
        self.inner.column_count()
    }

    fn columns(&self) -> Vec<Column> {
        self.inner.columns()
    }
}

