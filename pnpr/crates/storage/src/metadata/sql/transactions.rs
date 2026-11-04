//! How an operation runs: a write in a transaction that runs again after a
//! conflict, and a read in a transaction on a source that meets its
//! freshness.

use super::{
    SqlMetadata,
    adapter::{Connection, ReadSource},
    cache::RenderCache,
    catalog_generation,
};
use pnpr_error::{RegistryError, Result};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// How many times a write runs before a conflict it keeps meeting fails it.
const WRITE_ATTEMPTS: u32 = 4;

/// The wait before the first rerun of a write that met a conflict, doubled
/// per rerun.
const FIRST_RETRY_DELAY: Duration = Duration::from_millis(10);

/// How current a read must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Freshness {
    /// What the last commit left: a read that decides a write, answers a
    /// retry of one, or authorizes one.
    Current,
    /// A view a replica may serve while it is within its freshness bound.
    Bounded,
    /// A view at this catalog generation or later, such as one a client
    /// sent as the minimum it has seen.
    AtLeast(u64),
}

/// How a read of the replica went.
enum ReplicaRead<Output> {
    Read(Output),
    /// The replica has not reached the generation the read requires.
    Behind,
    /// The replica failed where the primary may not; a sync would not help.
    Failed,
}

impl SqlMetadata {
    /// Run the write `operation` on the blocking pool.
    pub(super) async fn write<Output, Operation>(
        self: &Arc<Self>,
        operation: Operation,
    ) -> Result<Output>
    where
        Output: Send + 'static,
        Operation: FnMut(&dyn Connection) -> Result<Output> + Send + 'static,
    {
        let database = Arc::clone(self);
        tokio::task::spawn_blocking(move || database.run_write(operation)).await?
    }

    /// Run `operation`, which reads `catalog`, on the blocking pool in a read
    /// transaction that meets `freshness`.
    pub(super) async fn read<Output, Operation>(
        self: &Arc<Self>,
        catalog: String,
        freshness: Freshness,
        operation: Operation,
    ) -> Result<Output>
    where
        Output: Send + 'static,
        Operation: FnMut(&dyn Connection, &RenderCache) -> Result<Output> + Send + 'static,
    {
        let database = Arc::clone(self);
        tokio::task::spawn_blocking(move || database.run_read(&catalog, freshness, operation))
            .await?
    }

    /// Run the write `operation`, again from its start while the adapter
    /// reports a conflict it may clear.
    fn run_write<Output>(
        &self,
        mut operation: impl FnMut(&dyn Connection) -> Result<Output>,
    ) -> Result<Output> {
        self.ensure_schema()?;
        self.retry_conflicts(|| self.write_once(&mut operation))
    }

    /// Run `write`, again from its start while the adapter reports a conflict
    /// it may clear, up to [`WRITE_ATTEMPTS`] runs spaced from
    /// [`FIRST_RETRY_DELAY`]. `write` must be safe to run again.
    pub(super) fn retry_conflicts<Output>(
        &self,
        mut write: impl FnMut() -> Result<Output>,
    ) -> Result<Output> {
        let mut delay = FIRST_RETRY_DELAY;
        let mut attempt = 1;
        loop {
            match write() {
                Err(error) if attempt < WRITE_ATTEMPTS && self.adapter.is_retryable(&error) => {
                    std::thread::sleep(delay);
                    delay *= 2;
                    attempt += 1;
                }
                result => return result,
            }
        }
    }

    /// Run the write `operation` once.
    fn write_once<Output>(
        &self,
        operation: &mut impl FnMut(&dyn Connection) -> Result<Output>,
    ) -> Result<Output> {
        let mut output = None;
        self.adapter.write(&mut |connection| {
            output = Some(operation(connection)?);
            Ok(())
        })?;
        output.ok_or_else(|| RegistryError::Internal {
            reason: "a metadata write did not run".to_string(),
        })
    }

    /// Run `operation`, which reads `catalog`, in one read transaction that
    /// meets `freshness`.
    fn run_read<Output>(
        &self,
        catalog: &str,
        freshness: Freshness,
        mut operation: impl FnMut(&dyn Connection, &RenderCache) -> Result<Output>,
    ) -> Result<Output> {
        self.ensure_schema()?;
        let minimum = match freshness {
            Freshness::Current => None,
            Freshness::Bounded => Some(0),
            Freshness::AtLeast(generation) => Some(generation),
        };
        let required = match minimum {
            Some(minimum) if self.adapter.has_replica() => minimum,
            _ => return self.read_primary(&mut operation),
        };
        let started = Instant::now();
        let behind = Some((catalog, required));
        match self.read_replica(behind, &mut operation)? {
            ReplicaRead::Read(output) => return Ok(output),
            // A sync the first read made already counts.
            ReplicaRead::Behind if self.adapter.sync_replica(started).is_ok() => {
                if let ReplicaRead::Read(output) = self.read_replica(behind, &mut operation)? {
                    return Ok(output);
                }
            }
            ReplicaRead::Behind | ReplicaRead::Failed => {}
        }
        self.read_primary(&mut operation)
    }

    /// Run `operation` on the replica once it has reached `required`.
    fn read_replica<Output>(
        &self,
        required: Option<(&str, u64)>,
        operation: &mut impl FnMut(&dyn Connection, &RenderCache) -> Result<Output>,
    ) -> Result<ReplicaRead<Output>> {
        match self.read_once(ReadSource::Replica, required, operation) {
            Ok(Some(output)) => Ok(ReplicaRead::Read(output)),
            Ok(None) => Ok(ReplicaRead::Behind),
            Err(error) if self.adapter.is_database_failure(&error) => {
                tracing::warn!(%error, "a metadata read of the replica failed; reading the primary");
                Ok(ReplicaRead::Failed)
            }
            Err(error) => Err(error),
        }
    }

    fn read_primary<Output>(
        &self,
        operation: &mut impl FnMut(&dyn Connection, &RenderCache) -> Result<Output>,
    ) -> Result<Output> {
        self.read_once(ReadSource::Primary, None, operation)?
            .ok_or_else(|| RegistryError::Internal {
                reason: "a metadata read of the primary did not run".to_string(),
            })
    }

    /// Run `operation` once on `source` in one read transaction. `None` when
    /// the source has not reached `required`, a catalog and the generation
    /// it must be at, so the operation did not run.
    fn read_once<Output>(
        &self,
        source: ReadSource,
        required: Option<(&str, u64)>,
        operation: &mut impl FnMut(&dyn Connection, &RenderCache) -> Result<Output>,
    ) -> Result<Option<Output>> {
        let mut output = None;
        self.adapter.read(source, &mut |connection| {
            connection.begin_read()?;
            let result = self.read_if_reached(connection, required, operation);
            let ended = if result.is_ok() { connection.commit() } else { connection.rollback() };
            output = result?;
            ended
        })?;
        Ok(output)
    }

    /// Run `operation` on `connection` when it has reached `required`.
    fn read_if_reached<Output>(
        &self,
        connection: &dyn Connection,
        required: Option<(&str, u64)>,
        operation: &mut impl FnMut(&dyn Connection, &RenderCache) -> Result<Output>,
    ) -> Result<Option<Output>> {
        if let Some((catalog, generation)) = required
            && generation > 0
            && catalog_generation(connection, catalog)? < generation
        {
            return Ok(None);
        }
        operation(connection, &self.cache).map(Some)
    }
}
