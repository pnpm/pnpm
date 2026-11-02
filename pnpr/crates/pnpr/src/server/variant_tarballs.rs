//! The `<name>@<version>/variants/<tag>` route, and
//! `<name>@<version>+r<revision>/variants/<tag>` for a revision-aware
//! version: a hosted platform variant's tar (see
//! [`super::publishing::variants`]).

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
    (raw_name, release, tag): (&str, &str, &str),
) -> Response {
    let (version, revision) = super::split_packuments::record_address(release);
    let (storage, package) =
        match variant_storage(state, identity, registry, (raw_name, version, tag)) {
            Ok(found) => found,
            Err(response) => return response,
        };
    let (body, len, unpacked_size) =
        match open_variant(&storage, &package, (version, revision, tag)).await {
            Ok(Some(found)) => found,
            Ok(None) => return not_found(),
            Err(err) => return err.into_response(),
        };
    let source = TarSource {
        name: None,
        tgz_integrity: None,
        keeps_content: false,
        trust: TgzTrust::Verified,
        tar_integrity_known: true,
        decode_limit: tar_decode_limit(unpacked_size),
    };
    revision_tarball_response(state, headers, body, len, RevisionResource::Gzip(source)).await
}

/// A hosted variant's stored `.tgz`, its length, and its tar's unpacked
/// size, or `None` when the version has no such variant.
async fn open_variant(
    storage: &Storage,
    package: &CanonicalPackageName,
    (version, revision, tag): (&str, Option<u64>, &str),
) -> Result<Option<(axum::body::Body, Option<u64>, Option<u64>)>, RegistryError> {
    let Some((label, unpacked_size)) =
        hosted_variant(storage, package, version, revision, tag).await?
    else {
        return Ok(None);
    };
    let filename = variants::variant_blob_name(package, version, &label);
    Ok(storage
        .open_hosted_blob(package, &filename)
        .await?
        .map(|(body, len)| (body, len, unpacked_size)))
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
    if !pnpr_package_name::is_safe_path_segment(version)
        || !pnpm_platform_variants::is_valid_variant_tag(tag)
    {
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

/// The attachment label a hosted version's variant `tag` is stored under,
/// and its unpacked size, or `None` when the version has no such variant.
/// Addressed by revision, the variant is served only while that revision is
/// the selected one: a hosted version's variant set is not replaced by a
/// revision.
async fn hosted_variant(
    storage: &Storage,
    package: &CanonicalPackageName,
    version: &str,
    revision: Option<u64>,
    tag: &str,
) -> Result<Option<(String, Option<u64>)>, RegistryError> {
    let Some(bytes) = storage.read_hosted_document(package).await? else {
        return Ok(None);
    };
    let packument = serde_json::from_slice::<Value>(&bytes)?;
    Ok(packument
        .get("versions")
        .and_then(|versions| versions.get(version))
        .filter(|manifest| {
            revision.is_none_or(|revision| {
                revision == super::split_packuments::selected_revision(manifest)
            })
        })
        .and_then(|manifest| variants::stored_variant_label(manifest, tag)))
}
