//! Changing the stored registry graph: every admin write, from a whole
//! document to one team member, is an edit of the stored document that is
//! validated in full, written as the next revision, and served on this
//! replica before the response.

use super::{
    super::{AppState, RegistryError, managed_state::WRITE_ATTEMPTS},
    RoutingSnapshot,
    reload::publish_routing,
    stored::{
        Credentials, check_revision, graph_version, occupied_namespace_error, parse_document,
        stored_document,
    },
};
use crate::audit::Actor;
use pnpr_config::{RegistryGraphChanges, RegistryGraphDocument, RoutingConfig};
use pnpr_storage::metadata::{
    HostedNamespace, NewRegistryGraph, REGISTRY_GRAPH_REVISIONS_KEPT, RegistryGraphWrite,
};
use std::sync::Arc;

/// The version the caller expects the stored graph to have.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Expected<'a> {
    /// Apply the edit to whatever is stored, rereading and reapplying it when
    /// another writer gets in first.
    Latest,
    /// Apply it only while the stored graph has one of these versions
    /// (`If-Match`).
    OneOf(&'a [String]),
}

/// Whether the edit is stored or only checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Effect {
    Write,
    DryRun,
}

/// What the edit does with the stored document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    /// Changes part of it, so it must be readable.
    Part,
    /// Replaces all of it, so an unreadable stored document can be repaired.
    /// The checks then compare against the newest readable revision.
    Whole,
}

/// One edit of the stored graph.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GraphEdit<'a> {
    pub(crate) actor: Actor<'a>,
    pub(crate) expected: Expected<'a>,
    pub(crate) effect: Effect,
    pub(crate) scope: Scope,
    /// The revision a restore copies.
    pub(crate) restored_from: Option<u64>,
}

impl<'a> GraphEdit<'a> {
    /// A write by `actor` against the latest stored graph.
    pub(crate) fn latest(actor: Actor<'a>) -> Self {
        Self {
            actor,
            expected: Expected::Latest,
            effect: Effect::Write,
            scope: Scope::Part,
            restored_from: None,
        }
    }

    /// Only while the stored graph has one of `versions`, when given
    /// (`If-Match`).
    pub(crate) fn expecting(mut self, versions: Option<&'a [String]>) -> Self {
        self.expected = versions.map_or(Expected::Latest, Expected::OneOf);
        self
    }
}

/// What an edit did.
#[derive(Clone)]
pub(crate) struct EditOutcome {
    /// The stored version before the edit.
    pub(crate) previous_version: String,
    /// The stored version after it. Equal to `previous_version` for a dry run
    /// and for an edit that changes nothing, which stores no revision.
    pub(crate) version: String,
    pub(crate) changes: RegistryGraphChanges,
    /// The routing of `version`, which this replica serves once the edit
    /// returns. For a dry run, the routing it served before.
    pub(crate) routing: Arc<RoutingSnapshot>,
}

impl EditOutcome {
    pub(crate) fn wrote(&self) -> bool {
        self.version != self.previous_version
    }
}

/// Apply `edit` to the stored graph. See the module docs.
pub(crate) async fn edit_graph<Edit>(
    state: &AppState,
    request: GraphEdit<'_>,
    edit: Edit,
) -> Result<EditOutcome, RegistryError>
where
    Edit: Fn(&mut RegistryGraphDocument) -> Result<(), RegistryError> + Send + Sync,
{
    let _reload = state.inner.identity.managed.reload.lock().await;
    for _ in 0..WRITE_ATTEMPTS {
        let attempt = try_edit(state, request, &edit).await?;
        if let Some(outcome) = attempt {
            return Ok(outcome);
        }
    }
    Err(RegistryError::AdminConflict {
        reason: "the registries kept changing; retry the request".to_string(),
    })
}

/// The stored graph an edit starts from.
struct Base {
    generation: Option<u64>,
    version: String,
    document: RegistryGraphDocument,
    /// Whether `document` is the stored one, rather than the newest readable
    /// revision standing in for an unreadable one.
    is_stored: bool,
}

/// One attempt. `None` when another writer stored a revision first.
async fn try_edit<Edit>(
    state: &AppState,
    request: GraphEdit<'_>,
    edit: &Edit,
) -> Result<Option<EditOutcome>, RegistryError>
where
    Edit: Fn(&mut RegistryGraphDocument) -> Result<(), RegistryError> + Send + Sync,
{
    let base = read_base(state, request).await?;
    let mut next = base.document.clone();
    edit(&mut next)?;
    let changes = next.changes_since(&base.document);
    let config = &state.inner.config;
    let checked =
        check_revision(config, &state.inner.storage, &base.document, &next, Credentials::Resolve)
            .await?;
    let unchanged =
        base.is_stored && serde_json::to_value(&base.document)? == serde_json::to_value(&next)?;
    if request.effect == Effect::DryRun || unchanged {
        let routing = match request.effect {
            Effect::DryRun => state.inner.current_routing(),
            Effect::Write => serve_generation(state, checked.routing, base.generation),
        };
        let version = base.version.clone();
        return Ok(Some(EditOutcome { previous_version: base.version, version, changes, routing }));
    }
    let target = Target { current: base.generation, vacated: &checked.vacated };
    let Some(generation) = store_revision(state, request, target, &next).await? else {
        return Ok(None);
    };
    let routing = publish_routing(state, checked.routing, Some(generation));
    let version = graph_version(Some(generation));
    Ok(Some(EditOutcome { previous_version: base.version, version, changes, routing }))
}

/// The routing of the stored generation `generation`, published from
/// `routing` when this replica serves an older one, so an edit that stored
/// nothing still answers with the version this replica serves.
fn serve_generation(
    state: &AppState,
    routing: RoutingConfig,
    generation: Option<u64>,
) -> Arc<RoutingSnapshot> {
    let current = state.inner.current_routing();
    if current.generation == generation {
        return current;
    }
    publish_routing(state, routing, generation)
}

async fn read_base(state: &AppState, request: GraphEdit<'_>) -> Result<Base, RegistryError> {
    let stored = state.inner.storage.read_registry_graph().await?;
    let generation = stored.as_ref().map(|stored| stored.generation);
    let version = graph_version(generation);
    if let Expected::OneOf(versions) = request.expected {
        check_version(&version, versions)?;
    }
    let (document, is_stored) = match (stored_document(stored.as_ref()), request.scope) {
        (Ok(document), _) => (document, true),
        (Err(_), Scope::Whole) => (newest_readable_document(state, generation).await?, false),
        (Err(err), Scope::Part) => return Err(err),
    };
    Ok(Base { generation, version, document, is_stored })
}

/// Where a revision is written: after the stored generation `current`, and
/// only while the namespaces it vacates are empty.
#[derive(Clone, Copy)]
struct Target<'a> {
    current: Option<u64>,
    vacated: &'a [HostedNamespace],
}

/// Store `next` as the revision after `target.current`. `None` when another
/// writer stored one first and the edit may be retried.
async fn store_revision(
    state: &AppState,
    request: GraphEdit<'_>,
    target: Target<'_>,
    next: &RegistryGraphDocument,
) -> Result<Option<u64>, RegistryError> {
    let document = serde_json::to_string(next)?;
    let revision = NewRegistryGraph {
        document: &document,
        actor_id: request.actor.id,
        restored_from: request.restored_from,
        vacated: target.vacated,
    };
    match state.inner.storage.write_registry_graph(target.current, revision).await? {
        RegistryGraphWrite::Written(generation) => Ok(Some(generation)),
        RegistryGraphWrite::Conflict { .. } => match request.expected {
            Expected::Latest => Ok(None),
            Expected::OneOf(versions) => Err(precondition_failed(versions)),
        },
        RegistryGraphWrite::HostedNotEmpty(occupied) => Err(occupied_namespace_error(&occupied)),
    }
}

/// The newest stored revision older than `current` that parses, or the empty
/// graph.
async fn newest_readable_document(
    state: &AppState,
    current: Option<u64>,
) -> Result<RegistryGraphDocument, RegistryError> {
    let storage = &state.inner.storage;
    let limit = u32::try_from(REGISTRY_GRAPH_REVISIONS_KEPT).unwrap_or(u32::MAX);
    let older = storage.list_registry_graph_revisions(current, limit).await?;
    for revision in older {
        let Some(stored) = storage.read_registry_graph_revision(revision.generation).await? else {
            continue;
        };
        if let Ok(document) = parse_document(&stored.document) {
            return Ok(document);
        }
    }
    Ok(RegistryGraphDocument::default())
}

fn check_version(current: &str, expected: &[String]) -> Result<(), RegistryError> {
    if expected
        .iter()
        .any(|version| version == current)
    {
        return Ok(());
    }
    Err(precondition_failed(expected))
}

fn precondition_failed(expected: &[String]) -> RegistryError {
    RegistryError::PreconditionFailed {
        resource: "the registries".to_string(),
        expected: expected.join(", "),
    }
}
