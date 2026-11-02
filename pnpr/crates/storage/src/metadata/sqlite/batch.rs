//! One write transaction of a hosted catalog, committed under the catalog's
//! next generation.

use super::{PackageKey, advance_catalog, catalog_generation, commit::Commit, to_column};
use pnpr_error::Result;
use rusqlite::{Connection, Transaction, TransactionBehavior, params};

/// A write transaction that commits every package it changes under one new
/// generation of its catalog, or nothing.
pub(super) struct Batch<'conn> {
    transaction: Transaction<'conn>,
    catalog: String,
    generation: u64,
    /// The packages the batch changed.
    touched: Vec<PackageKey>,
}

impl<'conn> Batch<'conn> {
    /// Begin the batch, taking the database's write lock, so the generation
    /// read here is the one the commit replaces.
    pub(super) fn begin(connection: &'conn mut Connection, catalog: &str) -> Result<Self> {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation = catalog_generation(&transaction, catalog)? + 1;
        Ok(Self { transaction, catalog: catalog.to_string(), generation, touched: Vec::new() })
    }

    pub(super) fn transaction(&self) -> &Transaction<'conn> {
        &self.transaction
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
        Commit { transaction: &self.transaction, key, generation: self.generation }
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
        advance_catalog(&self.transaction, &self.catalog, self.generation)?;
        self.transaction.commit()?;
        Ok(Some(self.generation))
    }
}
