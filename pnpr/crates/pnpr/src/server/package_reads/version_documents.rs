//! A version's document, `<base>/<name>/<version-or-tag>`, and its release
//! records, `<base>/<name>/<version>+r<revision>`.

use super::{
    AppState,
    CanonicalPackageName,
    Ecosystem,
    Identity,
    RegistryError,
    RegistrySource,
    Response,
    Value,
    addressed_registry,
    caller_scoped,
    dist_view,
    extract_upstream_version_manifest,
    extract_version_manifest,
    hosted_read_namespace,
    is_osv_vulnerable_packument_version,
    not_found,
    read_source_packument,
    registry_endpoint,
    reload_source_packument_lacking_version,
    resolve_registry_source,
    resolve_version_or_tag,
    tar_integrities_of,
};
use axum::response::IntoResponse;

pub(in super::super) async fn serve_version_manifest(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    version_or_tag: &str,
) -> Response {
    let Some(target) = addressed_registry(state, registry, Ecosystem::Npm) else {
        return not_found();
    };
    let base = registry_endpoint(state, Ecosystem::Npm, registry);
    let response =
        serve_registry_version_manifest(state, identity, &target, raw_name, version_or_tag, &base)
            .await;
    caller_scoped(state, Ecosystem::Npm, registry, Some(raw_name), response)
}

/// Serve a single version's manifest (`GET <base>/<pkg>/<version-or-tag>`)
/// through the registry graph. Resolves the package to its one concrete origin,
/// loads that origin's packument, and extracts the requested version with its
/// `dist.tarball` rewritten onto the same origin's base. A revision in the
/// address asks for that revision's release record.
async fn serve_registry_version_manifest(
    state: &AppState,
    identity: &Identity,
    registry: &str,
    raw_name: &str,
    requested: &str,
    tarball_base: &str,
) -> Response {
    let name = match CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm) {
        Ok(name) => name,
        Err(err) => return err.into_response(),
    };
    let resolved_source = resolve_registry_source(state, registry, name.as_str());
    let (packument, version_or_tag, revision) =
        match read_version_document(state, identity, &resolved_source, &name, requested).await {
            Ok(document) => document,
            Err(response) => return response,
        };
    if osv_hides_version(state, &packument, name.as_str(), version_or_tag) {
        return not_found();
    }
    let tar_integrities = match tar_integrities_of(state, &resolved_source, &name).await {
        Ok(table) => table,
        Err(err) => return err.into_response(),
    };
    let routing = state.routing();
    let view = dist_view(state, &routing, registry, &resolved_source, tar_integrities.as_ref());
    render_version_document(&packument, &name, (version_or_tag, revision), view, tarball_base)
}

fn render_version_document(
    packument: &Value,
    name: &CanonicalPackageName,
    (version_or_tag, revision): (&str, Option<u64>),
    view: super::super::tar_registry::DistView<'_>,
    tarball_base: &str,
) -> Response {
    let manifest =
        version_manifest(packument, name, (version_or_tag, view.revision_registry), tarball_base);
    let Some(manifest) = manifest.filter(|manifest| addresses(manifest, version_or_tag, revision))
    else {
        return not_found();
    };
    let tars = view.tar_integrities.map(|table| (table.as_ref(), view.tar_registry));
    super::super::split_packuments::version_document_response(manifest, revision, tars)
}

async fn read_version_document<'a>(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    name: &CanonicalPackageName,
    requested: &'a str,
) -> Result<(Value, &'a str, Option<u64>), Response> {
    let address = super::super::split_packuments::record_address(requested);
    if let RegistrySource::Hosted(hosted) = source {
        return read_hosted_document(state, identity, hosted, name, requested, address).await;
    }
    let packument = read_whole_packument(state, identity, source, name, requested)
        .await
        .map_err(IntoResponse::into_response)?
        .ok_or_else(not_found)?;
    let (version, revision) =
        if lists_version(&packument, requested) { (requested, None) } else { address };
    Ok((packument, version, revision))
}

async fn read_hosted_document<'a>(
    state: &AppState,
    identity: &Identity,
    source: &str,
    name: &CanonicalPackageName,
    requested: &'a str,
    (version, revision): (&'a str, Option<u64>),
) -> Result<(Value, &'a str, Option<u64>), Response> {
    // Hosted publishes reject build metadata. Only a valid tag can shadow a record.
    if (revision.is_none() || pnpr_storage::database::validate_dist_tag(requested).is_ok())
        && let Some(packument) =
            read_hosted_version_packument(state, identity, source, name, requested)
                .await
                .map_err(IntoResponse::into_response)?
    {
        return Ok((packument, requested, None));
    }
    let revision = revision.ok_or_else(not_found)?;
    if let Some(response) =
        serve_accepted_record(state, identity, source, name, version, revision).await
    {
        return Err(response);
    }
    let packument = read_hosted_record_packument(state, identity, source, name, version)
        .await
        .map_err(IntoResponse::into_response)?
        .ok_or_else(not_found)?;
    Ok((packument, version, Some(revision)))
}

/// The manifest of `version_or_tag`, with revision tarballs linked on
/// `revision_registry` when an upstream serves the package.
fn version_manifest(
    packument: &serde_json::Value,
    name: &CanonicalPackageName,
    (version_or_tag, revision_registry): (&str, Option<&str>),
    tarball_base: &str,
) -> Option<serde_json::Value> {
    match revision_registry {
        Some(source_registry) => extract_upstream_version_manifest(
            packument,
            name,
            version_or_tag,
            source_registry,
            tarball_base,
        ),
        None => extract_version_manifest(packument, name, version_or_tag, tarball_base),
    }
}

/// The release record a hosted version was accepted with, served as the
/// metadata store keeps it, or `None` when the store keeps none for it.
async fn serve_accepted_record(
    state: &AppState,
    identity: &Identity,
    source: &str,
    name: &CanonicalPackageName,
    version: &str,
    revision: u64,
) -> Option<Response> {
    let namespace = match hosted_read_namespace(state, identity, source, name.as_str()) {
        Ok(namespace) => namespace,
        Err(err) => return Some(err.into_response()),
    };
    let stored = state.inner.storage
        .for_hosted(&namespace, pnpr_registry::Ecosystem::Npm)
        .read_release_record(name, version, revision)
        .await;
    let record = match stored {
        Ok(record) => record?,
        Err(err) => return Some(err.into_response()),
    };
    let hidden = state.inner.osv_index
        .as_ref()
        .is_some_and(|osv_index| osv_index.is_vulnerable(name.as_str(), version));
    if hidden {
        return Some(not_found());
    }
    Some(super::super::split_packuments::accepted_record_response(record))
}

/// Whether a request for `version_or_tag` may be answered with `manifest`. A
/// release record is addressed by an exact version: a dist-tag moves, and the
/// record never changes.
fn addresses(manifest: &Value, version_or_tag: &str, revision: Option<u64>) -> bool {
    revision.is_none() || manifest.get("version").and_then(Value::as_str) == Some(version_or_tag)
}

async fn read_whole_packument(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    name: &CanonicalPackageName,
    version_or_tag: &str,
) -> Result<Option<Value>, RegistryError> {
    let Some(bytes) = read_source_packument(state, identity, source, name).await? else {
        return Ok(None);
    };
    let packument: Value = serde_json::from_slice(&bytes)?;
    let (version, revision) = super::super::split_packuments::record_address(version_or_tag);
    if lists_version(&packument, version_or_tag)
        || revision.is_some() && lists_version(&packument, version)
    {
        return Ok(Some(packument));
    }
    match reload_source_packument_lacking_version(state, identity, source, name).await? {
        Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        None => Ok(Some(packument)),
    }
}

fn lists_version(packument: &Value, version_or_tag: &str) -> bool {
    let version = resolve_version_or_tag(packument, version_or_tag);
    packument
        .get("versions")
        .and_then(|versions| versions.get(version))
        .is_some()
}

async fn read_hosted_version_packument(
    state: &AppState,
    identity: &Identity,
    source: &str,
    name: &CanonicalPackageName,
    version_or_tag: &str,
) -> Result<Option<Value>, RegistryError> {
    let namespace = hosted_read_namespace(state, identity, source, name.as_str())?;
    let requested = version_or_tag.to_string();
    let lookup = state.inner.storage
        .for_hosted(&namespace, pnpr_registry::Ecosystem::Npm)
        .lookup_in_hosted_document(name, move |bytes| {
            let packument: Value = serde_json::from_slice(bytes)?;
            Ok(lists_version(&packument, &requested).then_some(packument))
        })
        .await?;
    Ok(lookup.found())
}

/// The packument of a hosted release record: only the addressed version,
/// read from the metadata store on its own, so a record never costs the
/// package view. What depends on the route, the OSV filter and the facts
/// about the archive kept outside the metadata store, is applied to it as to
/// a whole packument.
async fn read_hosted_record_packument(
    state: &AppState,
    identity: &Identity,
    source: &str,
    name: &CanonicalPackageName,
    version: &str,
) -> Result<Option<Value>, RegistryError> {
    let namespace = hosted_read_namespace(state, identity, source, name.as_str())?;
    let manifest = state.inner.storage
        .for_hosted(&namespace, pnpr_registry::Ecosystem::Npm)
        .read_hosted_version(name, version)
        .await?;
    let Some(manifest) = manifest else { return Ok(None) };
    let manifest: Value = serde_json::from_str(&manifest)?;
    Ok(Some(serde_json::json!({ "name": name.as_str(), "versions": { version: manifest } })))
}

fn osv_hides_version(
    state: &AppState,
    packument: &Value,
    package_name: &str,
    version_or_tag: &str,
) -> bool {
    state.inner.osv_index
        .as_ref()
        .is_some_and(|osv_index| {
            let resolved = resolve_version_or_tag(packument, version_or_tag);
            is_osv_vulnerable_packument_version(packument, package_name, resolved, osv_index)
        })
}
