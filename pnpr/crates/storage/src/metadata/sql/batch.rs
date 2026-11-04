//! One write transaction of a hosted catalog, committed under the catalog's
//! next generation.

use super::{
    PackageKey,
    adapter::{Connection, Query, Row},
    advance_catalog,
    commit::Commit,
    to_column, to_generation,
};
use crate::metadata::ArtifactMember;
use pnpr_error::{RegistryError, Result};
use std::{cell::RefCell, ops::Deref};

/// A write transaction, rolled back unless it is committed.
pub(super) struct WriteTransaction<'conn> {
    connection: &'conn dyn Connection,
    open: bool,
}

impl<'conn> WriteTransaction<'conn> {
    /// Begin the transaction, taking the database's write lock.
    pub(super) fn begin(connection: &'conn dyn Connection) -> Result<Self> {
        connection.begin_write()?;
        Ok(Self { connection, open: true })
    }

    /// Commit the transaction. A failed `COMMIT` can leave it open, so it
    /// stays marked open until the commit succeeds and is otherwise rolled
    /// back on drop.
    pub(super) fn commit(mut self) -> Result<()> {
        self.connection.commit()?;
        self.open = false;
        Ok(())
    }
}

impl<'conn> Deref for WriteTransaction<'conn> {
    type Target = dyn Connection + 'conn;

    fn deref(&self) -> &Self::Target {
        self.connection
    }
}

impl Drop for WriteTransaction<'_> {
    fn drop(&mut self) {
        if self.open {
            // A failed rollback leaves the connection in its transaction; the
            // adapter ends it before the connection's next use.
            drop(self.connection.rollback());
        }
    }
}

/// A write transaction that commits every package it changes under one new
/// generation of its catalog, or nothing.
pub(super) struct Batch<'conn> {
    transaction: WriteTransaction<'conn>,
    catalog: String,
    generation: u64,
    /// The packages the batch changed.
    touched: Vec<PackageKey>,
    /// The npm versions the batch accepted.
    accepted: RefCell<Vec<ArtifactMember>>,
}

impl<'conn> Batch<'conn> {
    /// Begin the batch, taking the database's write lock, so the generation
    /// read here is the one the commit replaces. `reads` run with the
    /// statements that begin it, so a remote database answers them in the
    /// same round trip, and their rows are returned.
    pub(super) fn begin_with(
        connection: &'conn dyn Connection,
        catalog: &str,
        reads: &[Query<'_>],
    ) -> Result<(Self, Vec<Vec<Row>>)> {
        let transaction = WriteTransaction::begin(connection)?;
        let mut queries = vec![Query::new(
            "SELECT generation FROM catalogs WHERE catalog = ?1",
            params![catalog],
        )];
        queries.extend_from_slice(reads);
        let mut results = transaction.query_all(&queries)?.into_iter();
        let current = results
            .next()
            .and_then(|rows| rows.into_iter().next())
            .map(|mut row| row.get(0).map(to_generation))
            .transpose()?
            .unwrap_or(0);
        let results: Vec<Vec<Row>> = results.collect();
        if results.len() != reads.len() {
            return Err(RegistryError::Internal {
                reason: "a metadata batch did not answer every read".to_string(),
            });
        }
        let batch = Self {
            transaction,
            catalog: catalog.to_string(),
            generation: current + 1,
            touched: Vec::new(),
            accepted: RefCell::new(Vec::new()),
        };
        Ok((batch, results))
    }

    pub(super) fn transaction(&self) -> &dyn Connection {
        &*self.transaction
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn catalog(&self) -> &str {
        &self.catalog
    }

    /// The writes to one package of the batch's catalog.
    pub(super) fn package<'batch>(&'batch self, key: &'batch PackageKey) -> Commit<'batch> {
        debug_assert_eq!(key.catalog, self.catalog, "a batch writes one catalog");
        Commit {
            transaction: self.transaction(),
            key,
            generation: self.generation,
            accepted: &self.accepted,
        }
    }

    /// The npm versions the batch accepted, ordered by ecosystem, name, and
    /// version, as publication members.
    pub(super) fn accepted(&self) -> Vec<ArtifactMember> {
        let mut accepted = self.accepted.borrow().clone();
        accepted.sort_by(|left, right| {
            (left.ecosystem.as_str(), &left.name, &left.version).cmp(&(
                right.ecosystem.as_str(),
                &right.name,
                &right.version,
            ))
        });
        accepted
    }

    /// Record that the batch changed `key`.
    pub(super) fn touch(&mut self, key: &PackageKey) {
        if !self.touched.contains(key) {
            self.touched.push(key.clone());
        }
    }

    pub(super) fn changed_anything(&self) -> bool {
        !self.touched.is_empty()
    }

    /// Commit under the batch's generation when it changed anything, and
    /// report that generation. A batch that changed nothing rolls back.
    pub(super) fn finish(self) -> Result<Option<u64>> {
        if self.touched.is_empty() {
            return Ok(None);
        }
        let generation = to_column(self.generation);
        for key in &self.touched {
            let (catalog, ecosystem, name) = key.params();
            self.transaction.execute(
                "UPDATE packages SET generation = ?4
                 WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3",
                params![catalog, ecosystem, name, generation],
            )?;
        }
        advance_catalog(&*self.transaction, &self.catalog, self.generation)?;
        self.transaction.commit()?;
        Ok(Some(self.generation))
    }
}
