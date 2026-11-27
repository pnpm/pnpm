use super::{
    AppState,
    AuthedCaller,
    TargetRegistry,
    ecosystem::{
        UpstreamDocument,
        addressed_registry,
        caller_scoped,
        load_upstream_document,
        mount_bases,
        serve_upstream_artifact,
        upstream_for,
    },
    not_found,
    resolve_ecosystem_source,
};
use axum::{
    Router,
    extract::{
        Path,
        State,
    },
    http::header,
    response::{
        IntoResponse,
        Response,
    },
    routing::get,
};
use pnpm_crypto_shasums_file::{
    PickFileChecksumError,
    pick_file_checksum_from_shasums_file,
};
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_registry::Ecosystem;
use pnpr_upstream::Upstream;
use std::collections::HashMap;

const ECOSYSTEM: Ecosystem = Ecosystem::Node;
const PACKAGE: &str = "node";
const SHASUMS: &str = "SHASUMS256.txt";

pub(super) fn routes() -> Router<AppState> {
    let mut router = Router::new();
    for base in mount_bases(ECOSYSTEM) {
        router = router.route(&format!("{base}/release/{{*path}}"), get(get_release));
    }
    router
}

async fn get_release(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(params): Path<HashMap<String, String>>,
) -> Response {
    let Some(path) = params.get("path") else { return not_found() };
    let Some(target) = addressed_registry(&state, registry.as_deref(), ECOSYSTEM) else {
        return not_found();
    };
    let response = release_response(&state, &identity, &target, path).await
        .unwrap_or_else(IntoResponse::into_response);
    caller_scoped(&state, ECOSYSTEM, registry.as_deref(), Some(PACKAGE), response)
}

async fn release_response(
    state: &AppState,
    identity: &Identity,
    target: &str,
    path: &str,
) -> Result<Response, RegistryError> {
    let key = CanonicalPackageName::from_stored(PACKAGE, ECOSYSTEM);
    let source = resolve_ecosystem_source(state, target, ECOSYSTEM, PACKAGE);
    let (upstream, namespace) = upstream_for(state, identity, &source, &key)?;
    if path == "index.json" {
        let bytes = document(state, &upstream, &namespace, path).await?;
        return Ok(([(header::CONTENT_TYPE, "application/json")], bytes).into_response());
    }
    let (version, filename) = path.split_once('/').ok_or(RegistryError::NotFound)?;
    if version
        .strip_prefix('v')
        .map(semver::Version::parse)
        .is_none_or(|parsed| parsed.is_err())
    {
        return Err(RegistryError::NotFound);
    }
    if matches!(filename, SHASUMS | "SHASUMS256.txt.sig") {
        let bytes = document(state, &upstream, &namespace, path).await?;
        let mime = if filename == SHASUMS { "text/plain" } else { "application/octet-stream" };
        return Ok(([(header::CONTENT_TYPE, mime)], bytes).into_response());
    }
    let basename = artifact_basename(version, filename).ok_or(RegistryError::NotFound)?;
    let integrity = artifact_integrity(state, &upstream, &namespace, version, filename).await?;
    Ok(serve_upstream_artifact(
        state,
        &upstream,
        &namespace,
        &key,
        basename,
        &upstream.document_url(path),
        &integrity,
    )
    .await)
}

async fn artifact_integrity(
    state: &AppState,
    upstream: &Upstream,
    namespace: &str,
    version: &str,
    filename: &str,
) -> Result<ssri::Integrity, RegistryError> {
    let manifest = format!("{version}/{SHASUMS}");
    let bytes = document(state, upstream, namespace, &manifest).await?;
    let invalid_manifest = |reason: String| RegistryError::UpstreamResponse {
        url: upstream.document_url(&manifest),
        reason,
    };
    let text =
        std::str::from_utf8(&bytes).map_err(|err| err.to_string()).map_err(invalid_manifest)?;
    let checksum = pick_file_checksum_from_shasums_file(text, filename)
        .map_err(|err| match err {
            PickFileChecksumError::NotFound { .. } => RegistryError::NotFound,
            PickFileChecksumError::Malformed { .. } => invalid_manifest(err.to_string()),
        })?;
    Ok(checksum.parse().expect("validated SHA-256 SRI"))
}

fn artifact_basename<'path>(version: &str, path: &'path str) -> Option<&'path str> {
    if matches!(path, "win-x86/node.lib" | "win-x64/node.lib" | "win-arm64/node.lib") {
        return Some("node.lib");
    }
    let tail = path
        .strip_prefix("node-")?
        .strip_prefix(version)?
        .strip_prefix('-')?;
    let platform = [".tar.gz", ".tar.xz", ".zip"]
        .iter()
        .find_map(|ext| tail.strip_suffix(ext))?;
    (!platform.is_empty()
        && platform
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'))
    .then_some(path)
}

async fn document(
    state: &AppState,
    upstream: &Upstream,
    namespace: &str,
    path: &str,
) -> Result<Vec<u8>, RegistryError> {
    let key = CanonicalPackageName::parse(&path.replace('/', "_"), ECOSYSTEM)?;
    let request =
        UpstreamDocument { name: &key, relative_path: path, accept: None, limit: 16 * 1024 * 1024 };
    load_upstream_document(state, upstream, namespace, request, |doc| Ok(doc.bytes))
        .await?
        .ok_or(RegistryError::NotFound)
}
