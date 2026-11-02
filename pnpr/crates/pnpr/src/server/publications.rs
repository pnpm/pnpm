//! `GET /-/publications/{org}/{id-or-name}` and
//! `GET /-/publications/{org}?member=<ecosystem>:<name>@<version>`: the
//! publications of an organization's hosted catalog.
//!
//! `-` names the catalog of the registries without an `org`. A caller sees
//! only the members it may read, and a publication with none answers 404, so
//! a publication never discloses a package. Publications are recorded only
//! when the metadata lives in `SQLite`.

use super::{
    AppState, AuthedCaller, HostedGate, Identity, hosted_gate, not_found, private_no_cache,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_registry::Ecosystem;
use pnpr_storage::metadata::{ArtifactMember, Publication, PublicationOrigin};
use serde::Deserialize;
use serde_json::{Value, json};

/// The path segment naming the catalog of registries without an `org`.
const DEFAULT_CATALOG: &str = "-";

pub(super) async fn serve_publication(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path((org, id_or_name)): Path<(String, String)>,
) -> Response {
    let catalog = catalog_of(&org);
    let Some(database) = state.inner.storage.sqlite_metadata() else { return not_found() };
    let found = database.publication(catalog, &id_or_name).await;
    private_no_cache(respond(&state, &identity, found))
}

#[derive(Deserialize)]
pub(super) struct MemberQuery {
    member: String,
}

pub(super) async fn serve_member_publication(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    Path(org): Path<String>,
    Query(query): Query<MemberQuery>,
) -> Response {
    let catalog = catalog_of(&org);
    let Some(database) = state.inner.storage.sqlite_metadata() else { return not_found() };
    let Some((name, version)) = parse_member(&query.member) else {
        return RegistryError::BadRequest {
            reason: "`member` must be <ecosystem>:<name>@<version>".to_string(),
        }
        .into_response();
    };
    let found = database.initial_publication(&name, catalog, version).await;
    private_no_cache(respond(&state, &identity, found))
}

fn catalog_of(org: &str) -> &str {
    if org == DEFAULT_CATALOG { "" } else { org }
}

/// `npm:@scope/name@1.0.0` as the package and its version.
fn parse_member(member: &str) -> Option<(CanonicalPackageName, &str)> {
    let (ecosystem, package) = member.split_once(':')?;
    let ecosystem = Ecosystem::all().find(|known| known.as_str() == ecosystem)?;
    let at = package.get(1..)?.rfind('@')? + 1;
    let (name, version) = (&package[..at], &package[at + 1..]);
    let name = CanonicalPackageName::parse(name, ecosystem).ok()?;
    (!version.is_empty()).then_some((name, version))
}

fn respond(
    state: &AppState,
    identity: &Identity,
    found: Result<Option<Publication>, RegistryError>,
) -> Response {
    match found {
        Ok(Some(publication)) => match visible(state, identity, publication) {
            Some(publication) => Json(publication_json(&publication)).into_response(),
            None => not_found(),
        },
        Ok(None) => not_found(),
        Err(err) => err.into_response(),
    }
}

/// The publication with only the members `identity` may read, or `None` when
/// it may read none of them.
fn visible(
    state: &AppState,
    identity: &Identity,
    mut publication: Publication,
) -> Option<Publication> {
    let catalog = publication.catalog.clone();
    publication.members.retain(|member| may_read(state, identity, &catalog, member));
    (!publication.members.is_empty()).then_some(publication)
}

/// Whether a hosted registry of `catalog` and the member's ecosystem lets
/// `identity` read the member's package.
fn may_read(state: &AppState, identity: &Identity, catalog: &str, member: &ArtifactMember) -> bool {
    let routing = &state.inner.config.routing;
    routing.hosted
        .iter()
        .any(|(source, hosted)| {
            hosted.org == catalog
                && routing.registries.ecosystem(source) == Some(member.ecosystem)
                && matches!(
                    hosted_gate(state, identity, source, &member.name),
                    HostedGate::Allowed(org) if org == catalog,
                )
        })
}

fn publication_json(publication: &Publication) -> Value {
    let members: Vec<Value> = publication.members
        .iter()
        .map(|member| {
            json!({
                "ecosystem": member.ecosystem.as_str(),
                "name": member.name,
                "version": member.version,
                "artifact": member.artifact,
            })
        })
        .collect();
    let origin = match publication.origin {
        PublicationOrigin::Published => "published",
        PublicationOrigin::Imported => "imported",
    };
    json!({
        "id": publication.id,
        "org": publication.catalog,
        "name": publication.name,
        "origin": origin,
        "publisher": publication.publisher,
        "committedAt": publication.committed_at,
        "members": members,
    })
}
