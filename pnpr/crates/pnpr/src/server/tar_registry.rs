//! Registries that declare `tarballs: tar`, and the `<name>@<version>`
//! tarball route every registry serves.
//!
//! A tar registry's metadata describes the gzip-decoded content of each
//! published `.tgz`, and its tarball route serves that tar, encoded for the
//! request by [`tar_response`]. Its hash is derived at publish or on the first
//! decode, and kept in the storage's tar-integrity table.

use super::{
    AppState, CanonicalPackageName, Ecosystem, HeaderMap, Identity, PackumentDists, RegistryError,
    RegistrySource, Response, StatusCode, TarSource, Value, addressed_registry, caller_scoped,
    not_found, read_source_packument, resolve_registry_source, revision_source_registry,
    serve_registry_tarball, tar_response, tarball_basename,
};
use axum::response::IntoResponse;
use ssri::{Algorithm, Integrity};
use std::collections::BTreeMap;

/// How a served document describes each version's tarball.
#[derive(Clone, Copy)]
pub(super) enum DistView<'a> {
    /// The published `.tgz`, at a URL on the addressed base. `revision_registry`
    /// is the upstream whose integrity-addressed revisions may be linked.
    Tgz { revision_registry: Option<&'a str> },
    /// The tar, by the integrities in this table (see
    /// [`pnpr_storage::Storage::read_tar_integrities`]).
    Tar(&'a BTreeMap<String, String>),
}

/// The tar-integrity table of `name` when `registry` is a tar registry.
pub(super) async fn tar_integrities_of(
    state: &AppState,
    registry: &str,
    name: &CanonicalPackageName,
) -> Result<Option<BTreeMap<String, String>>, RegistryError> {
    if !is_tar_registry(state, registry) {
        return Ok(None);
    }
    state.inner.storage.read_tar_integrities(name).await.map(Some)
}

/// How a document served through `registry` from `source` describes each
/// version's tarball, given the tar-integrity table of a tar registry.
pub(super) fn dist_view<'a>(
    state: &'a AppState,
    registry: &str,
    source: &RegistrySource,
    tar_integrities: Option<&'a BTreeMap<String, String>>,
) -> DistView<'a> {
    if let Some(table) = tar_integrities {
        return DistView::Tar(table);
    }
    let revision_registry = match source {
        RegistrySource::Upstream(source) => revision_source_registry(state, registry, source),
        RegistrySource::Hosted(_) | RegistrySource::Unclaimed | RegistrySource::NotFound => None,
    };
    DistView::Tgz { revision_registry }
}

/// Whether the registry `key` declares `tarballs: tar`.
pub(super) fn is_tar_registry(state: &AppState, key: &str) -> bool {
    state.inner.config.routing.tar_registries.contains(key)
}

/// Rewrite every version of a packument to describe its tar.
pub(super) fn describe_tars(doc: &mut Value, table: &BTreeMap<String, String>) {
    let Some(versions) = doc.get_mut("versions").and_then(Value::as_object_mut) else {
        return;
    };
    for version in versions.values_mut() {
        describe_tar(version, table);
    }
}

/// Rewrite one version manifest's `dist` to describe its tar: the tar's
/// integrity when it is known, and none of the fields that name or sign the
/// `.tgz`.
pub(super) fn describe_tar(version: &mut Value, table: &BTreeMap<String, String>) {
    let Some(dist) = version.get_mut("dist").and_then(Value::as_object_mut) else {
        return;
    };
    let tar_integrity = tgz_integrity_key(dist)
        .and_then(|key| table.get(&key))
        .cloned();
    for field in ["tarball", "shasum", "signatures", "npm-signature", "revision", "revisions"] {
        dist.remove(field);
    }
    match tar_integrity {
        Some(integrity) => {
            dist.insert("integrity".to_string(), Value::String(integrity));
        }
        None => {
            dist.remove("integrity");
        }
    }
}

/// The key a version's tar integrity is recorded under: its `.tgz`
/// integrity, preferring SRI and falling back to the legacy SHA-1.
pub(super) fn tgz_integrity_key(dist: &serde_json::Map<String, Value>) -> Option<String> {
    if let Some(integrity) = dist.get("integrity").and_then(Value::as_str) {
        return integrity
            .parse::<Integrity>()
            .ok()
            .map(|integrity| integrity.to_string());
    }
    let shasum = dist.get("shasum").and_then(Value::as_str)?;
    Integrity::from_hex(shasum, Algorithm::Sha1).ok().map(|integrity| integrity.to_string())
}

/// Split `<name>@<version>` into its parts. A package name holds `@` only as
/// its scope prefix, so the version follows the last `@` past the first byte.
pub(super) fn split_package_identifier(identifier: &str) -> Option<(&str, &str)> {
    let at = identifier
        .rfind('@')
        .filter(|at| *at > 0)?;
    let (name, version) = (&identifier[..at], &identifier[at + 1..]);
    (!version.is_empty() && !name.ends_with('/')).then_some((name, version))
}

/// `GET {base}/<name>@<version>`: the version's tarball, as the published
/// `.tgz` on a `.tgz` registry and as its tar on a tar registry.
pub(super) async fn serve_package_tarball(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    registry: Option<&str>,
    package: (&str, &str),
) -> Response {
    let (raw_name, version) = package;
    let Some(target) = addressed_registry(state, registry, Ecosystem::Npm) else {
        return not_found();
    };
    let response =
        serve_version_tarball(state, identity, headers, &target, raw_name, version).await;
    caller_scoped(state, Ecosystem::Npm, registry, Some(raw_name), response)
}

async fn serve_version_tarball(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    target: &str,
    raw_name: &str,
    version: &str,
) -> Response {
    let name = match CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm) {
        Ok(name) => name,
        Err(err) => return err.into_response(),
    };
    let source = resolve_registry_source(state, target, name.as_str());
    let packument = match read_source_packument(state, identity, &source, &name).await {
        Ok(Some(packument)) => packument,
        Ok(None) => return not_found(),
        Err(err) => return err.into_response(),
    };
    let Some(tgz) = version_tgz(&packument, version) else {
        return not_found();
    };
    let response = serve_registry_tarball(state, identity, target, raw_name, &tgz.filename).await;
    if !is_tar_registry(state, target) || response.status() != StatusCode::OK {
        return response;
    }
    let keeps_content = match &source {
        RegistrySource::Upstream(upstream) => {
            state.inner.proxy.upstreams.get(upstream).is_some_and(pnpr_upstream::Upstream::caches)
        }
        RegistrySource::Hosted(_) => true,
        RegistrySource::Unclaimed | RegistrySource::NotFound => false,
    };
    let source = TarSource { name, tgz_integrity: tgz.integrity_key, keeps_content };
    tar_response(state, headers, response, source).await
}

struct VersionTgz {
    filename: String,
    integrity_key: Option<String>,
}

fn version_tgz(packument: &[u8], version: &str) -> Option<VersionTgz> {
    let packument: PackumentDists = serde_json::from_slice(packument).ok()?;
    let dist = packument.versions.get(version)?.dist.as_ref()?;
    let filename = dist.tarball
        .as_deref()
        .and_then(tarball_basename)?
        .to_string();
    let mut fields = serde_json::Map::new();
    if let Some(integrity) = &dist.integrity {
        fields.insert("integrity".to_string(), Value::String(integrity.clone()));
    }
    if let Some(shasum) = &dist.shasum {
        fields.insert("shasum".to_string(), Value::String(shasum.clone()));
    }
    Some(VersionTgz { filename, integrity_key: tgz_integrity_key(&fields) })
}
