//! A version's document, `<base>/<name>/<version-or-tag>`, and its release
//! records, `<base>/<name>/<version>+r<revision>`.

use super::{
    AppState, CanonicalPackageName, Ecosystem, Identity, RegistryError, RegistrySource, Response,
    Value, addressed_registry, caller_scoped, dist_view, extract_upstream_version_manifest,
    extract_version_manifest, hosted_read_namespace, is_osv_vulnerable_packument_version,
    is_tar_registry, not_found, read_source_packument, registry_endpoint, resolve_registry_source,
    resolve_version_or_tag, tar_integrities_of,
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
    let address = super::super::split_packuments::record_address(version_or_tag);
    if let (version, Some(revision)) = address {
        let record = (raw_name, version, revision);
        if let Some(response) = serve_accepted_record(state, identity, &target, record).await {
            return caller_scoped(state, Ecosystem::Npm, registry, Some(raw_name), response);
        }
    }
    let response =
        serve_registry_version_manifest(state, identity, &target, raw_name, address, &base).await;
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
    (version_or_tag, revision): (&str, Option<u64>),
    tarball_base: &str,
) -> Response {
    let read =
        read_version_packument(state, identity, registry, raw_name, version_or_tag, revision);
    let (name, resolved_source, packument) = match read.await {
        Ok(read) => read,
        Err(response) => return response,
    };
    let tar_integrities = match tar_integrities_of(state, &resolved_source, &name).await {
        Ok(table) => table,
        Err(err) => return err.into_response(),
    };
    let view = dist_view(state, registry, &resolved_source, tar_integrities.as_ref());
    let manifest = match view.revision_registry {
        Some(source_registry) => extract_upstream_version_manifest(
            &packument,
            &name,
            version_or_tag,
            source_registry,
            tarball_base,
        ),
        None => extract_version_manifest(&packument, &name, version_or_tag, tarball_base),
    };
    let Some(manifest) = manifest.filter(|manifest| addresses(manifest, version_or_tag, revision))
    else {
        return not_found();
    };
    let tars = view.tar_integrities.map(|table| (table.as_ref(), view.tar_registry));
    super::super::split_packuments::version_document_response(manifest, revision, tars)
}

/// The release record a hosted version was accepted with, served as the
/// metadata store keeps it, or `None` when the store keeps none for it.
///
/// A registry that serves tars describes the version by its tar's
/// integrity, which is not the integrity the record was accepted with, so
/// it serves the record with that one field changed.
async fn serve_accepted_record(
    state: &AppState,
    identity: &Identity,
    registry: &str,
    (raw_name, version, revision): (&str, &str, u64),
) -> Option<Response> {
    let name = CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm).ok()?;
    let resolved_source = resolve_registry_source(state, registry, name.as_str());
    let RegistrySource::Hosted(source) = &resolved_source else { return None };
    let org = match hosted_read_namespace(state, identity, source, name.as_str()) {
        Ok(org) => org,
        Err(err) => return Some(err.into_response()),
    };
    let stored = state.inner.storage
        .for_hosted(&org, pnpr_registry::Ecosystem::Npm)
        .read_release_record(&name, version, revision)
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
    if !is_tar_registry(state, registry) {
        return Some(super::super::split_packuments::accepted_record_response(record, None));
    }
    let table = match tar_integrities_of(state, &resolved_source, &name).await {
        Ok(table) => table,
        Err(err) => return Some(err.into_response()),
    };
    let table = table.unwrap_or_default();
    Some(super::super::split_packuments::accepted_record_response(record, Some(&table)))
}

/// Whether a request for `version_or_tag` may be answered with `manifest`. A
/// release record is addressed by an exact version: a dist-tag moves, and the
/// record never changes.
fn addresses(manifest: &Value, version_or_tag: &str, revision: Option<u64>) -> bool {
    revision.is_none() || manifest.get("version").and_then(Value::as_str) == Some(version_or_tag)
}

/// The packument a version-manifest request reads, with the source it
/// resolved to, or the response that ends the request. A hosted release
/// record, addressed by `revision`, reads only its version (see
/// [`read_hosted_record_packument`]).
async fn read_version_packument(
    state: &AppState,
    identity: &Identity,
    registry: &str,
    raw_name: &str,
    version_or_tag: &str,
    revision: Option<u64>,
) -> Result<(CanonicalPackageName, RegistrySource, Value), Response> {
    let name = match CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm) {
        Ok(name) => name,
        Err(err) => return Err(err.into_response()),
    };
    let resolved_source = resolve_registry_source(state, registry, name.as_str());
    let packument = match (&resolved_source, revision) {
        (RegistrySource::Hosted(source), Some(_)) => {
            read_hosted_record_packument(state, identity, source, &name, version_or_tag).await
        }
        _ => read_whole_packument(state, identity, &resolved_source, &name).await,
    };
    let packument = match packument {
        Ok(Some(packument)) => packument,
        Ok(None) => return Err(not_found()),
        Err(err) => return Err(err.into_response()),
    };
    if osv_hides_version(state, &packument, name.as_str(), version_or_tag) {
        return Err(not_found());
    }
    Ok((name, resolved_source, packument))
}

async fn read_whole_packument(
    state: &AppState,
    identity: &Identity,
    source: &RegistrySource,
    name: &CanonicalPackageName,
) -> Result<Option<Value>, RegistryError> {
    let Some(bytes) = read_source_packument(state, identity, source, name).await? else {
        return Ok(None);
    };
    Ok(Some(serde_json::from_slice(&bytes)?))
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
    let org = hosted_read_namespace(state, identity, source, name.as_str())?;
    let manifest = state.inner.storage
        .for_hosted(&org, pnpr_registry::Ecosystem::Npm)
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
