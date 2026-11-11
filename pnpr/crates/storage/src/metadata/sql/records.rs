use super::{
    SqlMetadata,
    adapter::{Connection, Param, Query},
    batch::WriteTransaction,
    transactions::Freshness,
};
use crate::MetadataRecord;
use pnpr_error::{RegistryError, Result};
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

pub(crate) enum RecordKey {
    Registry { kind: String, name: String },
    Staged(String),
}

impl RecordKey {
    fn identity(&self) -> (&str, &str, Vec<Param<'_>>) {
        match self {
            Self::Registry { kind, name } => (
                "registry_records",
                "kind = ?1 AND name = ?2",
                vec![Param::Text(kind), Param::Text(name)],
            ),
            Self::Staged(id) => ("staged_metadata", "id = ?1", vec![Param::Text(id)]),
        }
    }
}

impl SqlMetadata {
    pub(crate) async fn read_record(
        self: &Arc<Self>,
        key: RecordKey,
    ) -> Result<Option<MetadataRecord>> {
        self.read(String::new(), Freshness::Current, move |connection, _| {
            let (table, predicate, parameters) = key.identity();
            connection
                .query_optional(
                    &format!("SELECT generation, body FROM {table} WHERE {predicate}"),
                    &parameters,
                )?
                .map(|mut row| Ok(MetadataRecord { generation: row.get(0)?, bytes: row.get(1)? }))
                .transpose()
        })
        .await
    }

    pub(crate) async fn write_record(
        self: &Arc<Self>,
        key: RecordKey,
        expected: Option<i64>,
        body: &[u8],
    ) -> std::result::Result<Option<MetadataRecord>, RecordWriteError> {
        let body = body.to_vec();
        let commit_started = Arc::new(AtomicBool::new(false));
        let committing = Arc::clone(&commit_started);
        self.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            let (table, predicate, mut parameters) = key.identity();
            let generation = next_generation(&*transaction, table, predicate, &parameters, expected)?;
            let Some(generation) = generation else { return Ok(None) };
            parameters.push(Param::Integer(generation));
            parameters.push(Param::Blob(&body));
            let statement = match key {
                RecordKey::Registry { .. } => "INSERT INTO registry_records (kind, name, generation, body) VALUES (?1, ?2, ?3, ?4) ON CONFLICT (kind, name) DO UPDATE SET generation = excluded.generation, body = excluded.body",
                RecordKey::Staged(_) => "INSERT INTO staged_metadata (id, generation, body) VALUES (?1, ?2, ?3) ON CONFLICT (id) DO UPDATE SET generation = excluded.generation, body = excluded.body",
            };
            transaction.execute(statement, &parameters)?;
            if matches!(key, RecordKey::Registry { .. }) {
                super::cdn::enqueue(&*transaction, "*")?;
            }
            committing.store(true, Ordering::Release);
            transaction.commit()?;
            Ok(Some(MetadataRecord { generation, bytes: body.clone() }))
        }).await.map_err(|error| RecordWriteError {
            error,
            may_have_committed: commit_started.load(Ordering::Acquire),
        })
    }

    pub(crate) async fn remove_record(self: &Arc<Self>, key: RecordKey) -> Result<bool> {
        self.write(move |connection| {
            let transaction = WriteTransaction::begin(connection)?;
            let (table, predicate, parameters) = key.identity();
            let exists = transaction
                .query_optional(&format!("SELECT 1 FROM {table} WHERE {predicate}"), &parameters)?
                .is_some();
            transaction.execute(&format!("DELETE FROM {table} WHERE {predicate}"), &parameters)?;
            if exists && matches!(key, RecordKey::Registry { .. }) {
                super::cdn::enqueue(&*transaction, "*")?;
            }
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

fn next_generation(
    connection: &dyn Connection,
    table: &str,
    predicate: &str,
    parameters: &[Param<'_>],
    expected: Option<i64>,
) -> Result<Option<i64>> {
    let statement = format!("SELECT generation FROM {table} WHERE {predicate}");
    let queries = [
        Query::new(&statement, parameters),
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
