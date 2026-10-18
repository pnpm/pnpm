//! Registries that declare `tarballs: tar`, and the `<name>@<version>`
//! tarball route every registry serves.
//!
//! A tar registry's metadata describes the gzip-decoded content of each
//! published `.tgz`, and its tarball route serves that tar, encoded for the
//! request by [`super::tar_responses::tar_response`]. Its hash is derived at publish or on the first
//! decode, and kept in the storage's tar-integrity table.

use super::{
    AppState, CanonicalPackageName, RegistryError, RegistrySource, Value, revision_source_registry,
};
use ssri::{Algorithm, Integrity};
use std::{collections::BTreeMap, sync::Arc};

/// How a served document describes each version's tarball.
#[derive(Clone, Copy)]
pub(super) struct DistView<'a> {
    /// The upstream whose integrity-addressed revisions may be linked. Every
    /// other version is described by its canonical resource.
    pub(super) revision_registry: Option<&'a str>,
    /// On a tar registry, the table its canonical resources are described by
    /// (see [`pnpr_storage::Storage::read_tar_integrities`]). `None` describes
    /// the published `.tgz`, at a URL on the addressed base.
    pub(super) tar_integrities: Option<&'a BTreeMap<String, String>>,
}

/// The tar-integrity table of `name` when `registry` is a tar registry and
/// `source` can serve the package.
pub(super) async fn tar_integrities_of(
    state: &AppState,
    registry: &str,
    source: &RegistrySource,
    name: &CanonicalPackageName,
) -> Result<Option<Arc<BTreeMap<String, String>>>, RegistryError> {
    let servable = matches!(source, RegistrySource::Upstream(_) | RegistrySource::Hosted(_));
    if !servable || !is_tar_registry(state, registry) {
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
    let revision_registry = match source {
        RegistrySource::Upstream(source) => revision_source_registry(state, registry, source),
        RegistrySource::Hosted(_) | RegistrySource::Unclaimed | RegistrySource::NotFound => None,
    };
    DistView { revision_registry, tar_integrities }
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
/// `.tgz`. A projected revision already names its tar by digest, so it is
/// kept as projected.
pub(super) fn describe_tar(version: &mut Value, table: &BTreeMap<String, String>) {
    let Some(dist) = version.get_mut("dist").and_then(Value::as_object_mut) else {
        return;
    };
    if dist.contains_key("revision") {
        return;
    }
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
