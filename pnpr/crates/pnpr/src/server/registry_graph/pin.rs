use super::{
    super::{AppState, RegistryError},
    RoutingSnapshot, is_registry_admin_path,
};
use axum::response::{IntoResponse, Response};
use std::{future::Future, sync::Arc};

tokio::task_local! {
    static PINNED: Arc<RoutingSnapshot>;
}

/// Run `serve`, the rest of a request for `path`, with the current routing
/// snapshot pinned, so every [`AppState::routing`] call on the request's task
/// reads the same graph.
///
/// A replica that cannot confirm it follows the stored graph answers `503`,
/// except on the endpoints that consult no registry: the health probes, which
/// report it themselves, the account and sign-in endpoints, and the registry
/// admin API, which is how an admin repairs the graph. A session identity
/// built without a graph holds no team membership.
pub(in super::super) async fn with_pinned_routing(
    state: &AppState,
    path: &str,
    serve: impl Future<Output = Response>,
) -> Response {
    let unavailable = state.inner.identity.managed.graph
        .as_ref()
        .is_some_and(|graph| !graph.is_available());
    if unavailable && !serves_without_graph(path) {
        return RegistryError::RegistriesUnavailable.into_response();
    }
    PINNED.scope(state.inner.current_routing(), serve).await
}

/// `work` with this task's pinned snapshot pinned on whatever task runs it, so
/// work a request spawns keeps reading the request's graph.
pub(crate) fn carry_pinned_routing<Work: Future>(work: Work) -> impl Future<Output = Work::Output> {
    let pinned = PINNED.try_with(Arc::clone).ok();
    async move {
        match pinned {
            Some(snapshot) => PINNED.scope(snapshot, work).await,
            None => work.await,
        }
    }
}

/// Whether a request for `path` is served while the replica cannot serve its
/// graph. See [`with_pinned_routing`].
pub(in super::super) fn serves_without_graph(path: &str) -> bool {
    if is_registry_admin_path(path)
        || path.starts_with("/-/pnpr/v0/admin/users")
        || path == "/-/pnpr/v0/sign-in"
        || path.starts_with("/-/oidc/")
    {
        return true;
    }
    let tail = path
        .strip_prefix("/npm")
        .filter(|tail| tail.starts_with('/'))
        .unwrap_or(path);
    let tail = tail
        .strip_prefix("/~")
        .and_then(|rest| {
            rest.find('/')
                .map(|slash| &rest[slash..])
        })
        .unwrap_or(tail);
    matches!(tail, "/-/ping" | "/-/ready" | "/-/live" | "/-/whoami" | "/-/npm/v1/user")
        || ["/-/user/", "/-/npm/v1/tokens", "/-/v1/login"]
            .iter()
            .any(|prefix| tail.starts_with(prefix))
}

impl AppState {
    /// The routing snapshot this request pinned, or the current one outside a
    /// request.
    pub(crate) fn routing(&self) -> Arc<RoutingSnapshot> {
        PINNED.try_with(Arc::clone).unwrap_or_else(|_| self.inner.current_routing())
    }
}

#[cfg(test)]
mod tests;
