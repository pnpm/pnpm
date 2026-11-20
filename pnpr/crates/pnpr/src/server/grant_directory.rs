//! This replica's copy of the instance's grants, which the database holds,
//! and their application to the registries it serves.

use super::{
    AppState,
    RegistryError,
    managed_state::read_with_deadline,
};
use pnpr_config::{
    GraphSource,
    RoutingConfig,
};
use pnpr_policy::{
    Action,
    Grant,
    GrantSpec,
};
use pnpr_storage::database::{
    GrantContent,
    GrantWrite,
    StoredGrant,
    StoredGrants,
};
use std::sync::{
    Arc,
    PoisonError,
    RwLock,
    atomic::{
        AtomicU64,
        Ordering,
    },
};

/// The key that names every registry in a grant.
pub(super) const EVERY_REGISTRY: &str = "*";

/// The grants as of one stored generation.
#[derive(Debug, Default)]
pub(super) struct Grants {
    /// `None` before the first read.
    pub(super) generation: Option<u64>,
    /// Sorted by name.
    pub(super) grants: Vec<StoredGrant>,
}

impl Grants {
    pub(super) fn named(&self, name: &str) -> Option<&StoredGrant> {
        self.grants
            .iter()
            .find(|grant| grant.content.name == name)
    }
}

/// The grants this replica last read. A failed read keeps them.
#[derive(Debug, Default)]
pub(super) struct GrantState {
    published: RwLock<Arc<Grants>>,
    transition: AtomicU64,
}

pub(super) struct GrantTransition<'a>(&'a AtomicU64);

impl Drop for GrantTransition<'_> {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Release);
    }
}

impl GrantState {
    pub(super) fn snapshot(&self) -> Arc<Grants> {
        Arc::clone(&self.published.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// Read the published grants generation only when no grant reload is
    /// currently applying rules or invalidating caches.
    pub(super) fn stable_generation(&self) -> Option<Option<u64>> {
        let before = self.transition.load(Ordering::Acquire);
        if !before.is_multiple_of(2) {
            return None;
        }
        let generation = self.snapshot().generation;
        let after = self.transition.load(Ordering::Acquire);
        (before == after).then_some(generation)
    }

    pub(super) fn begin_transition(&self) -> GrantTransition<'_> {
        self.transition.fetch_add(1, Ordering::AcqRel);
        GrantTransition(&self.transition)
    }

    fn publish(&self, grants: Grants) {
        *self.published.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(grants);
    }
}

/// The subjects of `content` with their actions parsed. A stored action this
/// version does not know gives nothing.
fn subjects(content: &GrantContent) -> Vec<(String, Action)> {
    content.subjects
        .iter()
        .filter_map(|(subject, action)| Some((subject.clone(), Action::parse(action)?)))
        .collect()
}

/// `content` as it applies to one registry.
pub(super) fn spec(content: &GrantContent) -> GrantSpec {
    GrantSpec {
        name: content.name.clone(),
        include: content.include.clone(),
        exclude: content.exclude.clone(),
        subjects: subjects(content),
    }
}

/// Cover every registry of `routing` with the grants of `grants` that name
/// it. A graph an embedder supplied in code keeps the rules it was built
/// with.
pub(super) fn apply(state: &AppState, grants: &Grants, routing: &RoutingConfig) {
    if state.inner.config.graph_source == GraphSource::Supplied {
        return;
    }
    let tables = routing.hosted
        .iter()
        .map(|(key, hosted)| (key, &hosted.rules))
        .chain(
            routing.upstreams
                .iter()
                .map(|(key, upstream)| (key, &upstream.rules)),
        );
    for (key, rules) in tables {
        let ecosystem = routing.registries.ecosystem(key).unwrap_or_default();
        let covering = grants.grants
            .iter()
            .filter(|grant| {
                grant.content.registries
                    .iter()
                    .any(|registry| registry == key || registry == EVERY_REGISTRY)
            })
            .filter_map(|grant| match Grant::compile(&spec(&grant.content), ecosystem) {
                Ok(grant) => Some(grant),
                Err(reason) => {
                    tracing::warn!(
                        grant = %grant.content.name,
                        registry = %key,
                        %reason,
                        "a grant does not apply to a registry it names",
                    );
                    None
                }
            })
            .collect();
        rules.set_grants(covering);
    }
}

/// Read the grants when their stored generation moved, and cover the served
/// registries with them. Reports whether this replica now holds the stored
/// grants.
pub(super) async fn reload_grants(state: &AppState) -> bool {
    let directory = &state.inner.identity.managed.grants;
    let known = directory.snapshot().generation;
    match read_with_deadline(state.inner.storage.read_grants(known)).await {
        Ok(Some(StoredGrants { generation, grants })) => {
            let grants = Grants { generation: Some(generation), grants };
            let _transition = directory.begin_transition();
            // The served registries, not the ones this request pinned: a
            // reload under a request must reach what later requests read.
            apply(state, &grants, &state.inner.current_routing());
            directory.publish(grants);
            if let Some(resolver) = state.inner.resolver_shared.get() {
                resolver.forget_results();
            }
            true
        }
        Ok(None) => true,
        Err(err) => {
            tracing::error!(error = %err, "could not read the grants; serving the last ones read");
            false
        }
    }
}

/// Follow the stored registry graph now, so a grant may name a registry
/// another replica just added.
pub(super) async fn reload_written_registries(state: &AppState) {
    let managed = &state.inner.identity.managed;
    let _reload = managed.reload.lock().await;
    if let Some(graph) = &managed.graph {
        super::registry_graph::reload_graph(state, graph).await;
    }
}

/// Publish a grant write this replica just made. Waits for a reload in
/// progress, which may have read the grants before the write.
pub(super) async fn reload_written_grants(state: &AppState) -> Result<(), RegistryError> {
    let _reload = state.inner.identity.managed.reload.lock().await;
    if reload_grants(state).await {
        return Ok(());
    }
    Err(RegistryError::StorageUnavailable {
        reason: "the grants were written but could not be read back".to_string(),
    })
}

/// The generation a grant write was stored as, or why it was refused, given
/// the generation it expected.
pub(super) fn written(write: GrantWrite, expected: Option<u64>) -> Result<u64, RegistryError> {
    match write {
        GrantWrite::Written(generation) => Ok(generation),
        GrantWrite::Conflict { .. } => Err(RegistryError::PreconditionFailed {
            resource: "the grants".to_string(),
            expected: expected.map(|expected| expected.to_string()).unwrap_or_default(),
        }),
        GrantWrite::NameTaken => {
            Err(RegistryError::AdminConflict { reason: "another grant has that name".to_string() })
        }
        GrantWrite::NotFound => Err(RegistryError::NotFound),
        GrantWrite::UnknownTeam(id) => Err(RegistryError::BadRequest {
            reason: format!("the grant names `team:{id}`, and no team has that ID"),
        }),
        GrantWrite::UnknownRegistry(registry) => Err(RegistryError::BadRequest {
            reason: format!("the grant names the registry {registry:?}, which does not exist"),
        }),
    }
}

/// Refuse a grant this server could not apply: a registry that does not
/// exist, a pattern its ecosystem does not have, a subject that is not an
/// access token, or an action that does not exist.
pub(super) fn validate(state: &AppState, content: &GrantContent) -> Result<(), RegistryError> {
    validate_for(&state.inner.current_routing(), content)
}

/// [`validate`] against the registries of `routing`.
pub(crate) fn validate_for(
    routing: &RoutingConfig,
    content: &GrantContent,
) -> Result<(), RegistryError> {
    let invalid = |reason: String| RegistryError::BadRequest { reason };
    if content.name.is_empty() || content.name.contains(char::is_whitespace) {
        return Err(invalid("a grant name is one word".to_string()));
    }
    if content.registries.is_empty() {
        return Err(invalid("a grant names at least one registry".to_string()));
    }
    for (subject, action) in &content.subjects {
        pnpr_config::validate_access_token(subject).map_err(invalid)?;
        if Action::parse(action).is_none() {
            return Err(invalid(format!("unknown action {action:?}")));
        }
    }
    let spec = spec(content);
    content.registries
        .iter()
        .try_for_each(|registry| applies_to(routing, &spec, registry))
        .map_err(invalid)
}

/// Whether the grant `spec` can cover `registry`: it exists, holds packages,
/// and its ecosystem has the grant's patterns. The error is the reason only.
fn applies_to(routing: &RoutingConfig, spec: &GrantSpec, registry: &str) -> Result<(), String> {
    if registry == EVERY_REGISTRY {
        if !(spec.include.is_empty() && spec.exclude.is_empty()) {
            return Err("a grant on every registry cannot select packages by pattern".to_string());
        }
        return Grant::compile(spec, pnpr_registry::Ecosystem::default()).map(drop);
    }
    if !(routing.hosted.contains_key(registry) || routing.upstreams.contains_key(registry)) {
        return Err(format!("{registry:?} is not a hosted or upstream registry"));
    }
    let ecosystem = routing.registries.ecosystem(registry).unwrap_or_default();
    Grant::compile(spec, ecosystem)
        .map(drop)
        .map_err(|reason| format!("registry {registry:?}: {reason}"))
}
