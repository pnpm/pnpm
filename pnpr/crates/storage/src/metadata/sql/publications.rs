//! Publications and the recorded outcomes of publication requests.
//!
//! A publication is the commit that introduced one or more artifacts. Its id
//! is the catalog generation it committed, which is unique within the catalog
//! and orders publications with every other commit.

use super::{
    PackageKey,
    adapter::{Connection, Param, Query, Row},
    batch::Batch,
    to_column, to_generation,
};
use crate::{
    metadata::{ArtifactMember, Publication, PublicationRequest, RecordedOutcome},
    publish::{iso_from_unix_millis, now_millis},
};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::Ecosystem;
use std::time::Duration;

/// How long a recorded outcome answers a retry of its request.
pub(super) const OUTCOME_RETENTION: Duration = Duration::from_hours(7 * 24);

fn retention_cutoff() -> i64 {
    now_millis() - i64::try_from(OUTCOME_RETENTION.as_millis()).unwrap_or(i64::MAX)
}

/// The retained outcome of an identical request whose published members have
/// not been removed since it committed.
pub(super) fn recorded_outcome(
    connection: &dyn Connection,
    catalog: &str,
    request: &PublicationRequest,
) -> Result<Option<RecordedOutcome>> {
    let query = claim_query(catalog, request);
    let Some(mut row) = connection.query_optional(query.sql, &query.params)? else {
        return Ok(None);
    };
    if row.get::<String>(0)? != request.request_digest {
        return Ok(None);
    }
    outcome(catalog, row)
}

/// The read of the request's idempotency claim, which [`claim`] checks
/// inside the batch.
pub(super) fn claim_query<'query>(
    catalog: &'query str,
    request: &'query PublicationRequest,
) -> Query<'query> {
    Query {
        sql: "SELECT request_digest, generation, publication,
                     NOT EXISTS (
                         SELECT 1 FROM publication_members AS member
                         WHERE member.catalog = outcome.catalog
                           AND member.publication = outcome.publication
                           AND EXISTS (
                               SELECT 1 FROM removed_versions AS removed
                               WHERE removed.catalog = member.catalog
                                 AND removed.ecosystem = member.ecosystem
                                 AND removed.name = member.name
                                 AND removed.version = member.version
                                 AND (member.ecosystem != 'oci'
                                      OR removed.revision_key = member.artifact)
                                 AND removed.generation > outcome.generation
                           )
                     )
              FROM publication_outcomes AS outcome
              WHERE catalog = ?1 AND principal = ?2 AND idempotency_key = ?3
                AND committed_at >= ?4",
        params: vec![
            Param::Text(catalog),
            Param::Text(&request.principal),
            Param::Text(&request.idempotency_key),
            Param::Integer(retention_cutoff()),
        ],
    }
}

/// Check the request's idempotency claim, read by [`claim_query`]: the
/// replayable outcome of an identical request, `None` when the request must
/// be evaluated again, or a conflict when the key was used for another request.
pub(super) fn claim(
    batch: &Batch<'_>,
    request: &PublicationRequest,
    rows: Vec<Row>,
) -> Result<Option<RecordedOutcome>> {
    let Some(mut stored) = rows.into_iter().next() else { return Ok(None) };
    let digest: String = stored.get(0)?;
    if digest != request.request_digest {
        return Err(RegistryError::AdminConflict {
            reason: "the idempotency key was already used for a different request".to_string(),
        });
    }
    outcome(batch.catalog(), stored)
}

fn outcome(catalog: &str, mut row: Row) -> Result<Option<RecordedOutcome>> {
    if row.get::<i64>(3)? == 0 {
        return Ok(None);
    }
    let generation = to_generation(row.get(1)?);
    let publication = row.get::<Option<i64>>(2)?.map(to_generation);
    Ok(Some(RecordedOutcome { catalog: catalog.to_string(), generation, publication }))
}

/// Record the publication the batch commits, when it introduced any
/// artifact, and the request's outcome. The request's principal is the
/// publisher; without a request, `publisher` is. `name_taken` reports that
/// the catalog already gave the request's publication name to another
/// publication, read by [`name_query`] inside the batch. Returns the
/// publication's id.
pub(super) fn record(
    batch: &Batch<'_>,
    request: Option<&PublicationRequest>,
    publisher: Option<&str>,
    artifacts: &[ArtifactMember],
    name_taken: bool,
) -> Result<Option<u64>> {
    let members = members_of(batch, artifacts);
    let publication = if members.is_empty() {
        None
    } else {
        let publisher = request.map_or(publisher, |request| Some(request.principal.as_str()));
        insert_publication(batch, request, publisher, &members, name_taken)?;
        Some(batch.generation())
    };
    if let Some(request) = request {
        record_outcome(batch, request, publication)?;
    }
    Ok(publication)
}

/// The members of the publication the batch records: the npm versions it
/// accepted, and the `artifacts` of other ecosystems.
pub(super) fn members_of(batch: &Batch<'_>, artifacts: &[ArtifactMember]) -> Vec<ArtifactMember> {
    let mut members = batch.accepted();
    members.extend(artifacts.iter().cloned());
    members
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
            params![retention_cutoff()],
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
                now_millis(),
            ],
        )?;
    Ok(())
}

/// The members `rows` name, without those of an ecosystem this pnpr does not
/// know.
fn members(rows: Vec<Row>) -> Result<Vec<ArtifactMember>> {
    let members = rows
        .into_iter()
        .map(member_row)
        .collect::<Result<Vec<_>>>()?;
    Ok(members.into_iter().flatten().collect())
}

fn member_row(mut row: Row) -> Result<Option<ArtifactMember>> {
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
    principal: Option<&str>,
    members: &[ArtifactMember],
    name_taken: bool,
) -> Result<()> {
    let id = to_column(batch.generation());
    let name = request.and_then(|request| request.name.as_deref());
    // The batch holds the write lock, so no other commit takes the name
    // between its read and this insert.
    if let Some(name) = name
        && name_taken
    {
        return Err(RegistryError::AdminConflict {
            reason: format!("a publication named {name} already exists"),
        });
    }
    batch
        .transaction()
        .execute(
            "INSERT INTO publications (catalog, id, name, principal, committed_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
            params![batch.catalog(), id, name, principal, now_millis()],
        )?;
    for member in members {
        batch
            .transaction()
            .execute(
                "INSERT INTO publication_members
               (catalog, publication, ecosystem, name, version, artifact)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT DO NOTHING",
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

/// The read of whether the catalog already gave the publication name
/// `name` to a publication, for [`record`].
pub(super) fn name_query<'query>(catalog: &'query str, name: &'query str) -> Query<'query> {
    query!("SELECT 1 FROM publications WHERE catalog = ?1 AND name = ?2", catalog, name)
}

/// The publication `id_or_name` names in `catalog`: a decimal number is an
/// id, since a publication name never is one, and anything else a name.
pub(super) fn publication(
    connection: &dyn Connection,
    catalog: &str,
    id_or_name: &str,
) -> Result<Option<Publication>> {
    let decimal = !id_or_name.is_empty() && id_or_name.bytes().all(|byte| byte.is_ascii_digit());
    let (by_id, by_name) =
        if decimal { (id_or_name.parse::<i64>().ok(), None) } else { (None, Some(id_or_name)) };
    let row = connection.query_optional(
        "SELECT id, name, principal, committed_at FROM publications
         WHERE catalog = ?1 AND (id = ?2 OR name = ?3)",
        params![catalog, by_id, by_name],
    )?;
    let Some(mut row) = row else { return Ok(None) };
    let publication = Publication {
        catalog: catalog.to_string(),
        id: to_generation(row.get(0)?),
        name: row.get(1)?,
        publisher: row.get(2)?,
        committed_at: iso_from_unix_millis(row.get(3)?),
        members: Vec::new(),
    };
    with_members(connection, publication).map(Some)
}

fn with_members(connection: &dyn Connection, mut publication: Publication) -> Result<Publication> {
    let rows = connection.query(
        "SELECT ecosystem, name, version, artifact FROM publication_members
         WHERE catalog = ?1 AND publication = ?2 ORDER BY ecosystem, name, version, artifact",
        params![publication.catalog, to_column(publication.id)],
    )?;
    publication.members = members(rows)?;
    Ok(publication)
}

/// The publication that introduced `version` of `key`.
pub(super) fn initial_publication(
    connection: &dyn Connection,
    key: &PackageKey,
    version: &str,
) -> Result<Option<Publication>> {
    let (catalog, ecosystem, name) = key.params();
    let id: Option<i64> = connection
        .query_one(
            "SELECT MIN(publication) FROM publication_members
             WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4",
            params![catalog, ecosystem, name, version],
        )?
        .get(0)?;
    id.map_or(Ok(None), |id| publication(connection, catalog, &id.to_string()))
}
