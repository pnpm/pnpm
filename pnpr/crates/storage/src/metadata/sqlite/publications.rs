//! Publications and the recorded outcomes of publication requests.
//!
//! A publication is the commit that introduced one or more artifacts. Its id
//! is the catalog generation it committed, which is unique within the catalog
//! and orders publications with every other commit.

use super::{PackageKey, batch::Batch, to_column, to_generation};
use crate::metadata::{
    ArtifactMember, Publication, PublicationOrigin, PublicationRequest, RecordedOutcome,
};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::Ecosystem;
use rusqlite::{Connection, OptionalExtension, ffi, params};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long a recorded outcome answers a retry of its request.
pub(super) const OUTCOME_RETENTION: Duration = Duration::from_hours(7 * 24);

fn now_secs() -> i64 {
    let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    i64::try_from(since_epoch.as_secs()).unwrap_or(i64::MAX)
}

fn retention_cutoff() -> i64 {
    now_secs() - i64::try_from(OUTCOME_RETENTION.as_secs()).unwrap_or(i64::MAX)
}

/// The recorded outcome of `principal`'s request under `key`, in any catalog,
/// when it is still retained and its request had `digest`.
pub(super) fn recorded_outcome(
    connection: &Connection,
    principal: &str,
    key: &str,
    digest: &str,
) -> Result<Option<RecordedOutcome>> {
    let outcome = connection
        .query_row(
            "SELECT catalog, generation, publication FROM publication_outcomes
             WHERE principal = ?1 AND idempotency_key = ?2 AND request_digest = ?3
               AND committed_at >= ?4",
            params![principal, key, digest, retention_cutoff()],
            |row| {
                Ok(RecordedOutcome {
                    catalog: row.get(0)?,
                    generation: to_generation(row.get(1)?),
                    publication: row.get::<_, Option<i64>>(2)?.map(to_generation),
                })
            },
        )
        .optional()?;
    Ok(outcome)
}

/// Check the request's idempotency claim inside the batch: the outcome of an
/// identical request already committed, `None` for a new one, or a conflict
/// when the key was used for another request.
pub(super) fn claim(
    batch: &Batch<'_>,
    request: &PublicationRequest,
) -> Result<Option<RecordedOutcome>> {
    let stored = batch
        .transaction()
        .query_row(
            "SELECT request_digest, generation, publication FROM publication_outcomes
             WHERE catalog = ?1 AND principal = ?2 AND idempotency_key = ?3 AND committed_at >= ?4",
            params![
                batch.catalog(),
                request.principal,
                request.idempotency_key,
                retention_cutoff()
            ],
            |row| {
                let digest: String = row.get(0)?;
                let generation = to_generation(row.get(1)?);
                Ok((digest, generation, row.get::<_, Option<i64>>(2)?.map(to_generation)))
            },
        )
        .optional()?;
    let Some((digest, generation, publication)) = stored else { return Ok(None) };
    if digest != request.request_digest {
        return Err(RegistryError::AdminConflict {
            reason: "the idempotency key was already used for a different request".to_string(),
        });
    }
    Ok(Some(RecordedOutcome { catalog: batch.catalog().to_string(), generation, publication }))
}

/// Record the publication the batch commits, when it introduced any
/// artifact, and the request's outcome. Returns the publication's id.
pub(super) fn record(
    batch: &Batch<'_>,
    request: Option<&PublicationRequest>,
    artifacts: &[ArtifactMember],
) -> Result<Option<u64>> {
    let mut members = accepted_members(batch)?;
    members.extend(artifacts.iter().cloned());
    let publication = if members.is_empty() {
        None
    } else {
        insert_publication(batch, request, &members)?;
        Some(batch.generation())
    };
    if let Some(request) = request {
        record_outcome(batch, request, publication)?;
    }
    Ok(publication)
}

fn record_outcome(
    batch: &Batch<'_>,
    request: &PublicationRequest,
    publication: Option<u64>,
) -> Result<()> {
    batch
        .transaction()
        .execute(
            "DELETE FROM publication_outcomes WHERE committed_at < ?1",
            [retention_cutoff()],
        )?;
    batch
        .transaction()
        .execute(
            "INSERT INTO publication_outcomes (catalog, principal, idempotency_key, request_digest,
                                           generation, publication, committed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (catalog, principal, idempotency_key) DO UPDATE SET
           request_digest = excluded.request_digest, generation = excluded.generation,
           publication = excluded.publication, committed_at = excluded.committed_at",
            params![
                batch.catalog(),
                request.principal,
                request.idempotency_key,
                request.request_digest,
                to_column(batch.generation()),
                publication.map(to_column),
                now_secs(),
            ],
        )?;
    Ok(())
}

/// The npm versions this batch accepted, as publication members.
fn accepted_members(batch: &Batch<'_>) -> Result<Vec<ArtifactMember>> {
    let mut statement = batch
        .transaction()
        .prepare_cached(
            "SELECT ecosystem, name, version, integrity FROM versions
         WHERE catalog = ?1 AND accepted_generation = ?2 ORDER BY ecosystem, name, version",
        )?;
    let members = statement
        .query_map(params![batch.catalog(), to_column(batch.generation())], member_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(members.into_iter().flatten().collect())
}

fn member_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Option<ArtifactMember>> {
    let ecosystem: String = row.get(0)?;
    let Some(ecosystem) = Ecosystem::all().find(|known| known.as_str() == ecosystem) else {
        return Ok(None);
    };
    Ok(Some(ArtifactMember {
        ecosystem,
        name: row.get(1)?,
        version: row.get(2)?,
        artifact: row.get(3)?,
    }))
}

fn insert_publication(
    batch: &Batch<'_>,
    request: Option<&PublicationRequest>,
    members: &[ArtifactMember],
) -> Result<()> {
    let id = to_column(batch.generation());
    let name = request.and_then(|request| request.name.as_deref());
    let principal = request.map(|request| request.principal.as_str());
    let inserted = batch
        .transaction()
        .execute(
            "INSERT INTO publications (catalog, id, name, principal, committed_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
            params![batch.catalog(), id, name, principal, crate::publish::now_iso()],
        );
    if let Err(rusqlite::Error::SqliteFailure(error, _)) = &inserted
        && error.extended_code == ffi::SQLITE_CONSTRAINT_UNIQUE
    {
        return Err(RegistryError::AdminConflict {
            reason: format!("a publication named {} already exists", name.unwrap_or_default()),
        });
    }
    inserted?;
    for member in members {
        batch
            .transaction()
            .execute(
                "INSERT OR IGNORE INTO publication_members
               (catalog, publication, ecosystem, name, version, artifact)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    batch.catalog(),
                    id,
                    member.ecosystem.as_str(),
                    member.name,
                    member.version,
                    member.artifact,
                ],
            )?;
    }
    Ok(())
}

/// The publication `id_or_name` names in `catalog`.
pub(super) fn publication(
    connection: &Connection,
    catalog: &str,
    id_or_name: &str,
) -> Result<Option<Publication>> {
    let by_id = id_or_name.parse::<i64>().ok();
    let row = connection
        .query_row(
            "SELECT id, name, principal, committed_at FROM publications
             WHERE catalog = ?1 AND (id = ?2 OR name = ?3)
             ORDER BY id = ?2 DESC LIMIT 1",
            params![catalog, by_id, id_or_name],
            |row| {
                Ok(Publication {
                    catalog: catalog.to_string(),
                    id: to_generation(row.get(0)?),
                    name: row.get(1)?,
                    publisher: row.get(2)?,
                    committed_at: row.get(3)?,
                    origin: PublicationOrigin::Published,
                    members: Vec::new(),
                })
            },
        )
        .optional()?;
    row.map(|publication| with_members(connection, publication)).transpose()
}

fn with_members(connection: &Connection, mut publication: Publication) -> Result<Publication> {
    let mut statement = connection.prepare_cached(
        "SELECT ecosystem, name, version, artifact FROM publication_members
         WHERE catalog = ?1 AND publication = ?2 ORDER BY ecosystem, name, version, artifact",
    )?;
    let members = statement
        .query_map(params![publication.catalog, to_column(publication.id)], member_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    publication.members = members.into_iter().flatten().collect();
    Ok(publication)
}

/// The publication that introduced `version` of `key`. A version accepted
/// before publications were recorded gets a synthetic one, marked
/// [`PublicationOrigin::Imported`].
pub(super) fn initial_publication(
    connection: &Connection,
    key: &PackageKey,
    version: &str,
) -> Result<Option<Publication>> {
    let (catalog, ecosystem, name) = key.params();
    let id: Option<i64> = connection.query_row(
        "SELECT MIN(publication) FROM publication_members
             WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4",
        params![catalog, ecosystem, name, version],
        |row| row.get(0),
    )?;
    match id {
        Some(id) => publication(connection, catalog, &id.to_string()),
        None => imported_publication(connection, key, version),
    }
}

/// A synthetic publication for a version accepted outside any recorded one.
/// Its publisher is unknown and it proves nothing about how the version
/// arrived.
fn imported_publication(
    connection: &Connection,
    key: &PackageKey,
    version: &str,
) -> Result<Option<Publication>> {
    let (catalog, ecosystem, name) = key.params();
    let found = connection
        .query_row(
            "SELECT accepted_generation, integrity, published_at FROM versions
             WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4
             ORDER BY accepted_generation LIMIT 1",
            params![catalog, ecosystem, name, version],
            |row| {
                Ok((
                    to_generation(row.get(0)?),
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    Ok(found.map(|(id, artifact, published_at)| Publication {
        catalog: catalog.to_string(),
        id,
        name: None,
        publisher: None,
        committed_at: published_at.unwrap_or_default(),
        origin: PublicationOrigin::Imported,
        members: vec![ArtifactMember {
            ecosystem: key.ecosystem,
            name: name.to_string(),
            version: version.to_string(),
            artifact,
        }],
    }))
}
