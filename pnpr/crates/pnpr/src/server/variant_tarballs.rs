//! The `<name>@<version>/variants/<tag>` route: a hosted platform variant's
//! tar (see [`super::publishing::variants`]).

use super::{
    AppState, CanonicalPackageName, Ecosystem, HeaderMap, Identity, RegistryError, RegistrySource,
    Response, Storage, TarSource, TgzTrust, Value, addressed_registry, ensure_osv_allowed,
    hosted_read_namespace, not_found,
    publishing::variants,
    resolve_registry_source,
    revision_tarballs::{RevisionResource, revision_tarball_response},
    tar_decode_limit,
};
use axum::response::IntoResponse;

/// `GET <name>@<version>/variants/<tag>`: a hosted platform variant's tar.
pub(super) async fn serve_variant_tarball(
    state: &AppState,
    identity: &Identity,
    headers: &HeaderMap,
    registry: Option<&str>,
    (raw_name, version, tag): (&str, &str, &str),
) -> Response {
    let (storage, package) =
        match variant_storage(state, identity, registry, (raw_name, version, tag)) {
            Ok(found) => found,
            Err(response) => return response,
        };
    let declared = match hosted_variant(&storage, &package, version, tag).await {
        Ok(Some(declared)) => declared,
        Ok(None) => return not_found(),
        Err(err) => return err.into_response(),
    };
    let filename = variants::variant_blob_name(&package, version, tag);
    let (body, len) = match storage.open_hosted_blob(&package, &filename).await {
        Ok(Some(blob)) => blob,
        Ok(None) => return not_found(),
        Err(err) => return err.into_response(),
    };
    let source = TarSource {
        name: None,
        tgz_integrity: None,
        keeps_content: false,
        trust: TgzTrust::Verified,
        tar_integrity_known: true,
        decode_limit: tar_decode_limit(declared),
    };
    revision_tarball_response(state, headers, body, len, RevisionResource::Gzip(source)).await
}

/// The hosted storage a variant request reads, with its package, or the
/// response that ends the request.
fn variant_storage(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    (raw_name, version, tag): (&str, &str, &str),
) -> Result<(Storage, CanonicalPackageName), Response> {
    let Some(target) = addressed_registry(state, registry, Ecosystem::Npm) else {
        return Err(not_found());
    };
    let Ok(package) = CanonicalPackageName::parse(raw_name, Ecosystem::Npm) else {
        return Err(not_found());
    };
    if !pnpr_package_name::is_safe_path_segment(version) || !variants::is_valid_tag(tag) {
        return Err(not_found());
    }
    let RegistrySource::Hosted(source) = resolve_registry_source(state, &target, package.as_str())
    else {
        return Err(not_found());
    };
    let org = hosted_read_namespace(state, identity, &source, package.as_str())
        .map_err(IntoResponse::into_response)?;
    ensure_osv_allowed(state, &package, version).map_err(IntoResponse::into_response)?;
    Ok((state.inner.storage.for_hosted(&org, pnpr_registry::Ecosystem::Npm), package))
}

/// The unpacked size of a hosted version's variant `tag`, or `None` when the
/// version declares no such variant.
async fn hosted_variant(
    storage: &Storage,
    package: &CanonicalPackageName,
    version: &str,
    tag: &str,
) -> Result<Option<Option<u64>>, RegistryError> {
    let Some(bytes) = storage.read_hosted_document(package).await? else {
        return Ok(None);
    };
    let packument = serde_json::from_slice::<Value>(&bytes)?;
    let variant = packument
        .get("versions")
        .and_then(|versions| versions.get(version))
        .and_then(|manifest| manifest.get(variants::STORED_FIELD))
        .and_then(Value::as_array)
        .and_then(|variants| {
            variants
                .iter()
                .find(|variant| variant.get("tag").and_then(Value::as_str) == Some(tag))
        });
    Ok(variant.map(|variant| variant.get("unpackedSize").and_then(Value::as_u64)))
}
