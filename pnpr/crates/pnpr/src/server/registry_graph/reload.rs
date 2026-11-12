//! Following the stored registry graph: each replica polls the stored
//! generation, rebuilds its routing snapshot when the generation moves, and
//! stops serving once it has not been able to follow the graph for
//! [`STALE_LIMIT`].

use super::{
    super::{AppState, RegistryError, managed_state::read_with_deadline},
    build_snapshot,
    stored::{Credentials, check_file_references, stored_document, stored_routing},
};
use pnpr_config::RoutingConfig;
use pnpr_storage::metadata::StoredRegistryGraph;
use std::{
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::time::Instant;

/// How long a replica serves its last loaded graph after it last confirmed
/// that graph is the stored one.
pub(crate) const STALE_LIMIT: Duration = Duration::from_mins(1);

/// What a replica knows about the stored graph it follows.
pub(crate) struct GraphState {
    /// The stored generation the current snapshot was built from: `None` until
    /// the first load, `Some(None)` while nothing is stored. Only reloads read
    /// it.
    loaded: Mutex<Option<Option<u64>>>,
    /// When this replica last confirmed its snapshot is the stored graph, as
    /// milliseconds after `started` plus one; `0` before the first time. An
    /// atomic, so the request path takes no lock to check it.
    confirmed_at: AtomicU64,
    started: Instant,
}

impl GraphState {
    pub(crate) fn new() -> Self {
        Self { loaded: Mutex::new(None), confirmed_at: AtomicU64::new(0), started: Instant::now() }
    }

    /// Whether this replica may serve its snapshot: it confirmed the snapshot
    /// is the stored graph within the stale limit.
    pub(crate) fn is_available(&self) -> bool {
        match self.confirmed_at.load(Ordering::Acquire) {
            0 => false,
            confirmed => self.elapsed_millis() < confirmed + stale_limit_millis(),
        }
    }

    /// Record that the current snapshot is the stored generation `generation`.
    pub(crate) fn confirm(&self, generation: Option<u64>) {
        *self.loaded.lock().unwrap_or_else(PoisonError::into_inner) = Some(generation);
        self.confirmed_at.store(self.elapsed_millis() + 1, Ordering::Release);
    }

    fn loaded(&self) -> Option<Option<u64>> {
        *self.loaded.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn elapsed_millis(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

fn stale_limit_millis() -> u64 {
    u64::try_from(STALE_LIMIT.as_millis()).unwrap_or(u64::MAX)
}

/// Bring this replica's snapshot up to the stored graph. A graph that cannot
/// be read or built leaves the last loaded snapshot in place. Reports whether
/// the replica now serves the stored graph.
pub(crate) async fn reload_graph(state: &AppState, graph: &GraphState) -> bool {
    match read_with_deadline(poll_graph(state, graph.loaded())).await {
        Ok(Poll::Unchanged(generation)) => {
            graph.confirm(generation);
            true
        }
        Ok(Poll::Changed(stored)) => {
            let generation = stored.as_ref().map(|stored| stored.generation);
            match load_routing(state, stored.as_ref()) {
                Ok(routing) => {
                    publish_routing(state, routing, generation);
                    true
                }
                Err(err) => {
                    tracing::error!(
                        error = %err,
                        generation,
                        "could not build the stored registry graph; serving the last one loaded",
                    );
                    false
                }
            }
        }
        Err(err) => {
            tracing::error!(error = %err, "could not read the stored registry graph");
            false
        }
    }
}

/// Serve `routing`, built from the stored generation `generation`, from now
/// on.
pub(crate) fn publish_routing(state: &AppState, routing: RoutingConfig, generation: Option<u64>) {
    let inner = &state.inner;
    let previous = inner.current_routing();
    let mut snapshot =
        build_snapshot(&inner.config, routing, Some(&previous), &inner.upstream_guard_context);
    snapshot.generation = generation;
    inner.replace_routing(snapshot);
    if let Some(graph) = &inner.identity.managed.graph {
        graph.confirm(generation);
    }
}

enum Poll {
    Unchanged(Option<u64>),
    Changed(Option<StoredRegistryGraph>),
}

async fn poll_graph(state: &AppState, loaded: Option<Option<u64>>) -> Result<Poll, RegistryError> {
    let storage = &state.inner.storage;
    let current = storage.read_registry_graph_generation().await?;
    if loaded == Some(current) {
        return Ok(Poll::Unchanged(current));
    }
    Ok(Poll::Changed(storage.read_registry_graph().await?))
}

/// The routing of a stored revision. A config-file reference the graph no
/// longer satisfies is logged, not refused: writes refuse it, so only a
/// config change can cause it, and the replica should keep serving.
fn load_routing(
    state: &AppState,
    stored: Option<&StoredRegistryGraph>,
) -> Result<RoutingConfig, RegistryError> {
    let config = &state.inner.config;
    let document = stored_document(stored)?;
    let routing = stored_routing(config, &document, Credentials::Resolve)?;
    if let Err(err) = check_file_references(config, &routing) {
        tracing::warn!(error = %err, "the stored registry graph lacks what the config file names");
    }
    Ok(routing)
}
