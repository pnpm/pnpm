//! One write transaction of a hosted namespace, committed under the
//! namespace's next generation.

pub(super) use super::adapter::WriteTransaction;
use super::{
    PackageKey,
    adapter::{Connection, Query, Row},
    advance_namespace,
    commit::Commit,
    to_column, to_generation,
};
use crate::metadata::ArtifactMember;
use pnpr_error::{RegistryError, Result};
use std::cell::RefCell;

/// Whether a batch refuses to change a package of a hosted namespace that the
/// stored registry graph retired (see
/// [`crate::metadata::NewRegistryGraph::vacated`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Retired {
    Refused,
    Allowed,
}

/// A write transaction that commits every package it changes under one new
/// generation of its namespace, or nothing.
pub(super) struct Batch<'conn> {
    transaction: WriteTransaction<'conn>,
    namespace: String,
    generation: u64,
    cdn_enabled: bool,
    /// The packages the batch changed.
    touched: Vec<PackageKey>,
    /// The npm versions the batch accepted.
    accepted: RefCell<Vec<ArtifactMember>>,
    /// The ecosystems of the namespace that the batch must not change.
    retired: Vec<String>,
}

impl<'conn> Batch<'conn> {
    /// Begin the batch, taking the database's write lock, so the generation
    /// read here is the one the commit replaces. `reads` run with the
    /// statements that begin it, so a remote database answers them in the
    /// same round trip, and their rows are returned.
    pub(super) fn begin_with(
        connection: &'conn dyn Connection,
        namespace: &str,
        retired: Retired,
        reads: &[Query<'_>],
    ) -> Result<(Self, Vec<Vec<Row>>)> {
        let transaction = WriteTransaction::begin(connection)?;
        let mut queries = vec![Query::new(
            "SELECT COALESCE((SELECT generation FROM namespaces WHERE namespace = ?1), 0),
             EXISTS (SELECT 1 FROM cdn_subscriptions WHERE singleton = 1)",
            params![namespace],
        )];
        if retired == Retired::Refused {
            queries.push(Query::new(RETIRED_ECOSYSTEMS, params![namespace]));
        }
        queries.extend_from_slice(reads);
        let mut results = transaction.query_all(&queries)?.into_iter();
        let (current, cdn_enabled) = results
            .next()
            .and_then(|rows| rows.into_iter().next())
            .map(|mut row| {
                Ok::<_, RegistryError>((to_generation(row.get(0)?), row.get::<i64>(1)? != 0))
            })
            .transpose()?
            .unwrap_or((0, false));
        let retired = match retired {
            Retired::Refused => ecosystems_from(results.next().unwrap_or_default())?,
            Retired::Allowed => Vec::new(),
        };
        let results: Vec<Vec<Row>> = results.collect();
        if results.len() != reads.len() {
            return Err(RegistryError::Internal {
                reason: "a metadata batch did not answer every read".to_string(),
            });
        }
        let batch = Self {
            transaction,
            namespace: namespace.to_string(),
            generation: current + 1,
            cdn_enabled,
            touched: Vec::new(),
            accepted: RefCell::new(Vec::new()),
            retired,
        };
        Ok((batch, results))
    }

    pub(super) fn transaction(&self) -> &dyn Connection {
        &*self.transaction
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn namespace(&self) -> &str {
        &self.namespace
    }

    /// The writes to one package of the batch's namespace.
    pub(super) fn package<'batch>(&'batch self, key: &'batch PackageKey) -> Commit<'batch> {
        debug_assert_eq!(key.namespace, self.namespace, "a batch writes one namespace");
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
        if self.touched
            .iter()
            .any(|key| {
                self.retired
                    .iter()
                    .any(|retired| retired == key.ecosystem.as_str())
            })
        {
            // A replica that has not loaded the registry graph retiring the
            // namespace still routes publishes to it. Once it has, the
            // retried request no longer reaches the namespace.
            return Err(RegistryError::RegistriesUnavailable);
        }
        let generation = to_column(self.generation);
        for key in &self.touched {
            if self.cdn_enabled && key.ecosystem == pnpr_package_name::Ecosystem::Npm {
                super::cdn::enqueue(&*self.transaction, &key.name)?;
            }
            let (namespace, ecosystem, name) = key.params();
            self.transaction.execute(
                "UPDATE packages SET generation = ?4
                 WHERE namespace = ?1 AND ecosystem = ?2 AND name = ?3",
                params![namespace, ecosystem, name, generation],
            )?;
        }
        advance_namespace(&*self.transaction, &self.namespace, self.generation)?;
        self.transaction.commit()?;
        Ok(Some(self.generation))
    }
}

/// The ecosystems of a namespace (`?1`) that a batch must not change.
const RETIRED_ECOSYSTEMS: &str =
    "SELECT ecosystem FROM retired_hosted_namespaces WHERE namespace = ?1";

fn ecosystems_from(rows: Vec<Row>) -> Result<Vec<String>> {
    rows.into_iter()
        .map(|mut row| row.get::<String>(0))
        .collect()
}
