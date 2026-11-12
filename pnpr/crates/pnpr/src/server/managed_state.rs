use super::{
    AppState, Config, RegistryError,
    registry_graph::{GraphState, reload_graph},
    scim::{ScimState, reload_directory},
};
use pnpr_config::Management;
use std::{
    future::Future,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use tokio::{sync::OwnedMutexGuard, time::Instant};

const READ_DEADLINE: Duration = Duration::from_secs(5);

/// Minimum interval between successful request-triggered background reloads.
const STATE_TTL: Duration = Duration::from_secs(10);

/// How soon a replica tries again after a record it could not read.
const STATE_RETRY: Duration = Duration::from_secs(1);

/// How many times an edit rereads stored state after another writer replaced it
/// first.
pub(super) const WRITE_ATTEMPTS: usize = 5;

/// The state admins and identity providers manage at runtime: the registry
/// graph (`registriesManagedBy: api`) and the SCIM directory (`auth.scim`).
///
/// The metadata database holds both, so every replica sees the same state.
pub(super) struct ManagedState {
    /// The stored registry graph this replica follows. `None` when the config
    /// file declares the registries.
    pub(super) graph: Option<GraphState>,
    /// The account IDs the SCIM directory marks inactive. `None` without
    /// `auth.scim`.
    pub(super) scim: Option<ScimState>,
    /// When the state is next due for a reload. `None` before the first.
    due_at: Mutex<Option<Instant>>,
    /// Held by a reload and by every edit, so a reload that read the store
    /// before an edit cannot publish its older copy after it.
    pub(super) reload: Arc<tokio::sync::Mutex<()>>,
}

impl ManagedState {
    pub(super) fn new(config: &Config) -> Self {
        let graph = (config.registries_managed_by == Management::Api).then(GraphState::new);
        let scim = config.identity.auth.scim.as_ref().map(|_| ScimState::default());
        Self { graph, scim, due_at: Mutex::new(None), reload: Arc::default() }
    }

    fn is_empty(&self) -> bool {
        self.graph.is_none() && self.scim.is_none()
    }

    fn graph_is_stale(&self) -> bool {
        self.graph
            .as_ref()
            .is_some_and(|graph| !graph.is_available())
    }

    fn due_at(&self) -> Option<Instant> {
        *self.due_at.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn schedule(&self, after: Duration) {
        *self.due_at.lock().unwrap_or_else(PoisonError::into_inner) = Some(Instant::now() + after);
    }

    pub(super) fn schedule_retry(&self) {
        self.schedule(STATE_RETRY);
    }
}

/// Reload the managed state once it is due. The first load after startup
/// holds up the request, so no request is authorized against state this
/// replica has not read. Later reloads run in the background while requests
/// keep the current state.
pub(super) async fn refresh_managed_state(state: &AppState) {
    let managed = &state.inner.identity.managed;
    if managed.is_empty() {
        return;
    }
    match managed.due_at() {
        Some(due_at) if Instant::now() < due_at => {}
        // Past the stale limit the request could not be served from the
        // current state, so it waits for the reload as the first one does.
        Some(_) if managed.graph_is_stale() => {
            let guard = Arc::clone(&managed.reload).lock_owned().await;
            if managed.graph_is_stale() {
                reload_managed_state(state.clone(), guard).await;
            }
        }
        Some(_) => {
            if let Ok(guard) = Arc::clone(&managed.reload).try_lock_owned() {
                tokio::spawn(reload_managed_state(state.clone(), guard));
            }
        }
        None => {
            let guard = Arc::clone(&managed.reload).lock_owned().await;
            if managed.due_at().is_none() {
                reload_managed_state(state.clone(), guard).await;
            }
        }
    }
}

/// Reload the registry graph alongside the SCIM directory.
async fn reload_managed_state(state: AppState, _reload: OwnedMutexGuard<()>) {
    let managed = &state.inner.identity.managed;
    let directory = async {
        match &managed.scim {
            Some(scim) => reload_directory(&state, scim).await,
            None => true,
        }
    };
    let registries = async {
        match &managed.graph {
            Some(graph) => reload_graph(&state, graph).await,
            None => true,
        }
    };
    let (registries_read, scim_read) = tokio::join!(registries, directory);
    let all_read = registries_read && scim_read;
    managed.schedule(if all_read { STATE_TTL } else { STATE_RETRY });
}

/// Bound refresh reads without cancelling writes whose commit outcome could be unknown.
pub(super) async fn read_with_deadline<Output>(
    read: impl Future<Output = Result<Output, RegistryError>>,
) -> Result<Output, RegistryError> {
    tokio::time::timeout(READ_DEADLINE, read).await
        .map_err(|_| RegistryError::Internal {
            reason: "the managed authorization record read exceeded its deadline".to_string(),
        })?
}
