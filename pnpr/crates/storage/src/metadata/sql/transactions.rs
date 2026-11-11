//! How an operation runs: a write in a transaction that runs again after a
//! conflict, and a read in a transaction on a source that meets its
//! freshness.

use super::{
    SqlMetadata,
    adapter::{Connection, ReadSource, Snapshot},
    cache::RenderCache,
    namespace_generation,
};
use pnpr_error::{RegistryError, Result};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// How long after its first run a write that keeps meeting conflicts still
/// runs again.
const WRITE_RETRY_DEADLINE: Duration = Duration::from_secs(10);

/// The longest wait before the first rerun of a write that met a conflict,
/// doubled per rerun up to [`LAST_RETRY_DELAY`].
const FIRST_RETRY_DELAY: Duration = Duration::from_millis(10);

const LAST_RETRY_DELAY: Duration = Duration::from_millis(500);

/// How current a read must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Freshness {
    /// What the last commit left: a read that decides a write, answers a
    /// retry of one, or authorizes one.
    Current,
    /// A view a replica may serve while it is within its freshness bound.
    Bounded,
    /// A view at this namespace generation or later, such as one a client
    /// sent as the minimum it has seen.
    AtLeast(u64),
}

/// How a read of the replica went.
enum ReplicaRead<Output> {
    Read(Output, Snapshot),
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

    /// Run `operation`, which reads `namespace`, on the blocking pool in a read
    /// transaction that meets `freshness`.
    pub(super) async fn read<Output, Operation>(
        self: &Arc<Self>,
        namespace: String,
        freshness: Freshness,
        operation: Operation,
    ) -> Result<Output>
    where
        Output: Send + 'static,
        Operation: FnMut(&dyn Connection, &RenderCache) -> Result<Output> + Send + 'static,
    {
        let database = Arc::clone(self);
        tokio::task::spawn_blocking(move || database.run_read(&namespace, freshness, operation))
            .await?
    }

    /// [`Self::read`] at [`Freshness::Bounded`], reading the primary when a
    /// lagging replica's answer does not satisfy `present`.
    pub(super) async fn read_if<Output, Operation, Present>(
        self: &Arc<Self>,
        present: Present,
        operation: Operation,
    ) -> Result<Output>
    where
        Output: Send + 'static,
        Present: Fn(&Output) -> bool + Send + 'static,
        Operation: FnMut(&dyn Connection, &RenderCache) -> Result<Output> + Send + 'static,
    {
        let database = Arc::clone(self);
        tokio::task::spawn_blocking(move || database.run_read_if(present, operation)).await?
    }

    /// [`Self::read`] at [`Freshness::Bounded`], reading the primary when a
    /// lagging replica holds `None`.
    pub(super) async fn read_present<Output, Operation>(
        self: &Arc<Self>,
        operation: Operation,
    ) -> Result<Option<Output>>
    where
        Output: Send + 'static,
        Operation: FnMut(&dyn Connection, &RenderCache) -> Result<Option<Output>> + Send + 'static,
    {
        self.read_if(Option::is_some, operation).await
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
    /// it may clear and [`WRITE_RETRY_DEADLINE`] has not passed. Each rerun
    /// waits a random part of a delay that doubles from
    /// [`FIRST_RETRY_DELAY`], so writers that met each other do not meet
    /// again. `write` must be safe to run again.
    pub(super) fn retry_conflicts<Output>(
        &self,
        mut write: impl FnMut() -> Result<Output>,
    ) -> Result<Output> {
        let deadline = Instant::now() + WRITE_RETRY_DEADLINE;
        let mut delay = FIRST_RETRY_DELAY;
        loop {
            match write() {
                Err(error) if self.adapter.is_retryable(&error) => {
                    let wait = jittered(delay);
                    if Instant::now() + wait > deadline {
                        return Err(error);
                    }
                    std::thread::sleep(wait);
                    delay = (delay * 2).min(LAST_RETRY_DELAY);
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

    /// Run `operation`, which reads `namespace`, in one read transaction that
    /// meets `freshness`.
    fn run_read<Output>(
        &self,
        namespace: &str,
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
        let behind = Some((namespace, required));
        match self.read_replica(behind, &mut operation)? {
            ReplicaRead::Read(output, _) => return Ok(output),
            // A sync the first read made already counts.
            ReplicaRead::Behind if self.adapter.sync_replica(started).is_ok() => {
                if let ReplicaRead::Read(output, _) = self.read_replica(behind, &mut operation)? {
                    return Ok(output);
                }
            }
            ReplicaRead::Behind | ReplicaRead::Failed => {}
        }
        self.read_primary(&mut operation)
    }

    /// Run `operation` on the replica while it is within its freshness bound,
    /// and on the primary when a lagging replica's answer does not satisfy
    /// `present`.
    fn run_read_if<Output>(
        &self,
        present: impl Fn(&Output) -> bool,
        mut operation: impl FnMut(&dyn Connection, &RenderCache) -> Result<Output>,
    ) -> Result<Output> {
        self.ensure_schema()?;
        if !self.adapter.has_replica() {
            return self.read_primary(&mut operation);
        }
        match self.read_replica(None, &mut operation)? {
            ReplicaRead::Read(output, Snapshot::Current) => return Ok(output),
            ReplicaRead::Read(output, Snapshot::Lagging) if present(&output) => return Ok(output),
            ReplicaRead::Read(_, Snapshot::Lagging) | ReplicaRead::Behind | ReplicaRead::Failed => {
            }
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
            Ok((Some(output), snapshot)) => Ok(ReplicaRead::Read(output, snapshot)),
            Ok((None, _)) => Ok(ReplicaRead::Behind),
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
        self.read_once(ReadSource::Primary, None, operation)?.0
            .ok_or_else(|| RegistryError::Internal {
                reason: "a metadata read of the primary did not run".to_string(),
            })
    }

    /// Run `operation` once on `source` in one read transaction. `None` when
    /// the source has not reached `required`, a namespace and the generation
    /// it must be at, so the operation did not run.
    fn read_once<Output>(
        &self,
        source: ReadSource,
        required: Option<(&str, u64)>,
        operation: &mut impl FnMut(&dyn Connection, &RenderCache) -> Result<Output>,
    ) -> Result<(Option<Output>, Snapshot)> {
        let mut output = None;
        let snapshot = self.adapter.read(source, &mut |connection| {
            connection.begin_read()?;
            let result = self.read_if_reached(connection, required, operation);
            let ended = if result.is_ok() { connection.commit() } else { connection.rollback() };
            output = result?;
            ended
        })?;
        Ok((output, snapshot))
    }

    /// Run `operation` on `connection` when it has reached `required`.
    fn read_if_reached<Output>(
        &self,
        connection: &dyn Connection,
        required: Option<(&str, u64)>,
        operation: &mut impl FnMut(&dyn Connection, &RenderCache) -> Result<Output>,
    ) -> Result<Option<Output>> {
        if let Some((namespace, generation)) = required
            && generation > 0
            && namespace_generation(connection, namespace)? < generation
        {
            return Ok(None);
        }
        operation(connection, &self.cache).map(Some)
    }
}

/// A random wait between half of `delay` and all of it.
fn jittered(delay: Duration) -> Duration {
    let half = delay / 2;
    let random = getrandom::u64().unwrap_or_default();
    let spread = u64::try_from(half.as_micros()).unwrap_or(u64::MAX).max(1);
    half + Duration::from_micros(random % spread)
}
