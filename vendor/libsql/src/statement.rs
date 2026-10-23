use crate::params::IntoParams;
use crate::params::Params;
pub use crate::Column;
use crate::{Error, Result};
use crate::{Row, Rows};

#[async_trait::async_trait]
pub(crate) trait Stmt {
    fn finalize(&mut self);

    async fn execute(&self, params: &Params) -> Result<usize>;

    async fn query(&self, params: &Params) -> Result<Rows>;

    async fn run(&self, params: &Params) -> Result<()>;

    fn interrupt(&self) -> Result<()>;

    fn reset(&self);

    fn parameter_count(&self) -> usize;

    fn parameter_name(&self, idx: i32) -> Option<&str>;

    fn column_count(&self) -> usize;

    fn columns(&self) -> Vec<Column>;
}

/// A cached prepared statement.
pub struct Statement {
    pub(crate) inner: Box<dyn Stmt + Send + Sync>,
}

impl Statement {
    /// Finalize the cached statement.
    pub fn finalize(&mut self) {
        self.inner.finalize();
    }

    /// Execute queries on the statement, check [`Connection::execute`] for usage.
    pub async fn execute(&self, params: impl IntoParams) -> Result<usize> {
        tracing::trace!("execute for prepared statement");
        self.inner.execute(&params.into_params()?).await
    }

    /// Execute a query on the statement, check [`Connection::query`] for usage.
    pub async fn query(&self, params: impl IntoParams) -> Result<Rows> {
        tracing::trace!("query for prepared statement");
        self.inner.query(&params.into_params()?).await
    }

    /// Run a query on the statement.
    ///
    /// The `execute()` method returns an error if the query returns rows, which makes
    /// it unsuitable for running any type of SQL queries. Similarly, the `query()` method
    /// only works on SQL statements that return rows. Therefore, the `run()` method is
    /// provided to execute any type of SQL statement.
    ///
    /// Note: This is an extension to the Rusqlite API.
    pub async fn run(&self, params: impl IntoParams) -> Result<()> {
        tracing::trace!("run for prepared statement");
        self.inner.run(&params.into_params()?).await?;
        Ok(())
    }

    /// Interrupt the statement.
    pub fn interrupt(&self) -> Result<()> {
        self.inner.interrupt()
    }

    /// Execute a query that returns the first [`Row`].
    ///
    /// # Errors
    ///
    /// - Returns `QueryReturnedNoRows` if no rows were returned.
    pub async fn query_row(&mut self, params: impl IntoParams) -> Result<Row> {
        let mut rows = self.query(params).await?;

        let row = rows.next().await?.ok_or(Error::QueryReturnedNoRows)?;

        Ok(row)
    }

    /// Reset the state of this prepared statement.
    pub fn reset(&self) {
        self.inner.reset();
    }

    /// Fetch the amount of parameters in the prepared statement.
    pub fn parameter_count(&self) -> usize {
        self.inner.parameter_count()
    }

    /// Fetch the parameter name at the provided index.
    pub fn parameter_name(&self, idx: i32) -> Option<&str> {
        self.inner.parameter_name(idx)
    }

    /// Fetch the number of columns for the prepared statement.
    pub fn column_count(&self) -> usize {
        self.inner.column_count()
    }

    /// Fetch the list of columns for the prepared statement.
    pub fn columns(&self) -> Vec<Column> {
        self.inner.columns()
    }
}
