//! Committing a publication: every document of a request in one transaction
//! of its catalog, with the request's outcome.

use super::{Expected, PackageKey, batch::Batch, publications};
use crate::metadata::{
    ArtifactMember, BatchDocument, Generation, PublicationCommit, PublicationRequest,
};
use pnpr_error::{RegistryError, Result};
use rusqlite::Connection;

/// Commit every document under one new generation of `catalog`, or none of
/// them. A request whose identical twin already committed gets that commit's
/// outcome back and commits nothing.
pub(super) fn commit_publication(
    connection: &mut Connection,
    catalog: &str,
    documents: &[BatchDocument],
    artifacts: &[ArtifactMember],
    request: Option<&PublicationRequest>,
) -> Result<PublicationCommit> {
    let mut batch = Batch::begin(connection, catalog)?;
    if let Some(request) = request
        && let Some(outcome) = publications::claim(&batch, request)?
    {
        let generation = Generation { catalog: catalog.to_string(), value: outcome.generation };
        return Ok(PublicationCommit {
            generation,
            publication: outcome.publication,
            replayed: true,
        });
    }
    for document in documents {
        write_member(&mut batch, document)?;
    }
    let publication = if batch.changed_anything() {
        publications::record(&batch, request, artifacts)?
    } else {
        None
    };
    let value = batch.finish()?;
    let value = match value {
        Some(value) => value,
        None => super::catalog_generation(connection, catalog)?,
    };
    let generation = Generation { catalog: catalog.to_string(), value };
    Ok(PublicationCommit { generation, publication, replayed: false })
}

fn write_member(batch: &mut Batch<'_>, document: &BatchDocument) -> Result<()> {
    let key = PackageKey {
        catalog: batch.catalog().to_string(),
        ecosystem: document.ecosystem,
        name: document.name.clone(),
    };
    let expected = Expected::from_version(document.expected.as_ref())?;
    match batch.package(&key).write_document(&document.bytes, expected)? {
        None => Err(RegistryError::DocumentWriteConflict { package: document.name.clone() }),
        Some(true) => {
            batch.touch(&key);
            Ok(())
        }
        Some(false) => Ok(()),
    }
}
