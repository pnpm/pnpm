use super::{
    SqlMetadata,
    adapter::{Connection, Param, Query},
    batch::WriteTransaction,
    transactions::Freshness,
};
use crate::MetadataRecord;
use pnpr_error::{RegistryError, Result};
use pnpr_sql::DialectSql;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug)]
pub(crate) struct RecordWriteError {
    pub(crate) error: RegistryError,
    /// A commit may succeed even when its acknowledgement fails.
    pub(crate) may_have_committed: bool,
}

const STAGE_RECORD: DialectSql = DialectSql::with_mysql(
    "INSERT INTO staged_metadata (id, generation, body) VALUES (?1, ?2, ?3)
     ON CONFLICT (id) DO UPDATE SET generation = excluded.generation, body = excluded.body",
    "INSERT INTO staged_metadata (id, generation, body) VALUES (?1, ?2, ?3)
     AS excluded ON DUPLICATE KEY UPDATE generation = excluded.generation, body = excluded.body",
);

impl SqlMetadata {
    pub(crate) async fn read_staged_record(
        self: &Arc<Self>,
        id: &str,
    ) -> Result<Option<MetadataRecord>> {
        let id = id.to_string();
        self.read(String::new(), Freshness::Current, move |connection, _| {
            connection
                .query_optional(
                    "SELECT generation, body FROM staged_metadata WHERE id = ?1",
                    params![id],
                )?
                .map(|mut row| Ok(MetadataRecord { generation: row.get(0)?, bytes: row.get(1)? }))
                .transpose()
        })
        .await
    }

    pub(crate) async fn write_staged_record(
        self: &Arc<Self>,
        id: &str,
        expected: Option<i64>,
        body: &[u8],
    ) -> std::result::Result<Option<MetadataRecord>, RecordWriteError> {
        let id = id.to_string();
        let body = body.to_vec();
        let commit_started = Arc::new(AtomicBool::new(false));
        let committing = Arc::clone(&commit_started);
        self.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            let Some(generation) = next_staged_generation(&*transaction, &id, expected)? else {
                return Ok(None);
            };
            let parameters = [Param::Text(&id), Param::Integer(generation), Param::Blob(&body)];
            transaction.execute(STAGE_RECORD.get(transaction.dialect()), &parameters)?;
            committing.store(true, Ordering::Release);
            transaction.commit()?;
            Ok(Some(MetadataRecord { generation, bytes: body.clone() }))
        })
        .await
        .map_err(|error| RecordWriteError {
            error,
            may_have_committed: commit_started.load(Ordering::Acquire),
        })
    }

    pub(crate) async fn remove_staged_record(self: &Arc<Self>, id: &str) -> Result<bool> {
        let id = id.to_string();
        self.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            let exists = transaction
                .query_optional("SELECT 1 FROM staged_metadata WHERE id = ?1", params![id])?
                .is_some();
            transaction.execute("DELETE FROM staged_metadata WHERE id = ?1", params![id])?;
            transaction.commit()?;
            Ok(exists)
        })
        .await
    }

    pub(crate) async fn staged_metadata(self: &Arc<Self>) -> Result<Vec<(String, MetadataRecord)>> {
        self.read(String::new(), Freshness::Current, |connection, _| {
            connection
                .query("SELECT id, generation, body FROM staged_metadata", params![])?
                .into_iter()
                .map(|mut row| {
                    Ok((
                        row.get(0)?,
                        MetadataRecord { generation: row.get(1)?, bytes: row.get(2)? },
                    ))
                })
                .collect()
        })
        .await
    }

    pub(crate) async fn staged_ids(self: &Arc<Self>) -> Result<Vec<String>> {
        self.read(String::new(), Freshness::Current, |connection, _| {
            connection
                .query("SELECT id FROM staged_metadata", params![])?
                .into_iter()
                .map(|mut row| row.get(0))
                .collect()
        })
        .await
    }
}

pub(super) fn advance_record_generation(connection: &dyn Connection) -> Result<i64> {
    let mut row = connection
        .query_optional("SELECT generation FROM record_generation WHERE singleton = 1", params![])?
        .ok_or_else(|| RegistryError::Internal {
            reason: "the metadata record generation is missing".to_string(),
        })?;
    let generation = row.get::<i64>(0)? + 1;
    connection.execute(
        "UPDATE record_generation SET generation = ?1 WHERE singleton = 1",
        params![generation],
    )?;
    Ok(generation)
}

fn next_staged_generation(
    connection: &dyn Connection,
    id: &str,
    expected: Option<i64>,
) -> Result<Option<i64>> {
    let parameters = [Param::Text(id)];
    let queries = [
        Query::new("SELECT generation FROM staged_metadata WHERE id = ?1", &parameters),
        query!("SELECT generation FROM record_generation WHERE singleton = 1"),
    ];
    let mut results = connection.query_all(&queries)?.into_iter();
    let current = results
        .next()
        .unwrap_or_default()
        .into_iter()
        .next()
        .map(|mut row| row.get::<i64>(0))
        .transpose()?;
    if current != expected {
        return Ok(None);
    }
    let mut row = results
        .next()
        .unwrap_or_default()
        .into_iter()
        .next()
        .ok_or_else(|| RegistryError::Internal {
            reason: "the metadata record generation is missing".to_string(),
        })?;
    let generation = row.get::<i64>(0)? + 1;
    connection.execute(
        "UPDATE record_generation SET generation = ?1 WHERE singleton = 1",
        params![generation],
    )?;
    Ok(Some(generation))
}
