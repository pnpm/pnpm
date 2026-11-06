use super::{
    super::{require_admin, resolve_write_target_for},
    Action, AppState, AuthedCaller, CanonicalPackageName, ECOSYSTEM, HashMap, Identity, Path,
    ProjectDocument, RegistryError, RegistrySource, Response, State, StatusCode, TargetRegistry,
    Yanked, authorize, is_safe_path_segment, not_found, private_no_cache,
};
use axum::{Json, response::IntoResponse};
use pnpr_storage::{DOCUMENT_WRITE_RETRIES, DocumentUpdate};
use serde::Deserialize;

pub(super) const MAX_YANK_BODY_BYTES: usize = 8 * 1024;
const MAX_YANK_REASON_BYTES: usize = 1024;

#[derive(Deserialize)]
pub(super) struct YankRequest {
    reason: Option<String>,
}

pub(super) async fn yank(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(params): Path<HashMap<String, String>>,
    Json(request): Json<YankRequest>,
) -> Response {
    if request.reason
        .as_ref()
        .is_some_and(|reason| reason.len() > MAX_YANK_REASON_BYTES)
    {
        return private_no_cache(
            super::bad_request(format!("yank reason exceeds {MAX_YANK_REASON_BYTES} bytes"))
                .into_response(),
        );
    }
    change_file(
        &state,
        &identity,
        registry.as_deref(),
        &params,
        Some(
            request.reason
                .filter(|reason| !reason.is_empty())
                .map(Yanked::Reason)
                .unwrap_or(Yanked::Flag(true)),
        ),
    )
    .await
}

pub(super) async fn unyank(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(params): Path<HashMap<String, String>>,
) -> Response {
    change_file(&state, &identity, registry.as_deref(), &params, Some(Yanked::Flag(false))).await
}

pub(super) async fn delete_file(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(params): Path<HashMap<String, String>>,
) -> Response {
    change_file(&state, &identity, registry.as_deref(), &params, None).await
}

async fn change_file(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    params: &HashMap<String, String>,
    yanked: Option<Yanked>,
) -> Response {
    private_no_cache(match update_file(state, identity, registry, params, yanked).await {
        Ok(DocumentUpdate::Written) => StatusCode::OK.into_response(),
        Ok(DocumentUpdate::NotFound) => not_found(),
        Err(err) => err.into_response(),
    })
}

async fn update_file(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    params: &HashMap<String, String>,
    yanked: Option<Yanked>,
) -> Result<DocumentUpdate, RegistryError> {
    let project = params.get("project").ok_or(RegistryError::NotFound)?;
    let filename = params
        .get("filename")
        .filter(|name| is_safe_path_segment(name))
        .ok_or(RegistryError::NotFound)?;
    let key = CanonicalPackageName::parse(project, ECOSYSTEM)?;
    if yanked.is_none() {
        require_admin(state, identity, "delete", format!("Python file {filename:?}"))?;
    }
    let target = resolve_write_target_for(state, identity, registry, ECOSYSTEM, &key)?;
    if yanked.is_some() {
        authorize(
            state,
            identity,
            &RegistrySource::Hosted(target.source),
            key.as_str(),
            Action::Publish,
        )?;
    }
    let _guard = state.inner.locks.packages.lock(key.as_str()).await;
    state.inner.storage
        .for_hosted(&target.org, ECOSYSTEM)
        .update_hosted_document_with_retry(&key, DOCUMENT_WRITE_RETRIES, None, |existing| {
            update_document(existing, &key, filename, yanked.as_ref())
        })
        .await
}

fn update_document(
    existing: Option<&[u8]>,
    key: &CanonicalPackageName,
    filename: &str,
    yanked: Option<&Yanked>,
) -> Result<Option<Vec<u8>>, RegistryError> {
    let Some(bytes) = existing else { return Ok(None) };
    let mut document = ProjectDocument::parse(bytes)?;
    if document.format_version > pnpr_pypi::PROJECT_DOCUMENT_FORMAT_VERSION {
        return Err(RegistryError::DocumentFormatTooNew {
            document: key.as_str().to_string(),
            format_version: document.format_version,
            supported_version: pnpr_pypi::PROJECT_DOCUMENT_FORMAT_VERSION,
        });
    }
    let Some(index) = document.files
        .iter()
        .position(|file| file.filename == filename)
    else {
        return Ok(None);
    };
    if let Some(yanked) = yanked {
        document.files[index].yanked = yanked.clone();
    } else {
        document.files.remove(index);
        document.deleted_files.insert(filename.to_string());
        document.format_version = pnpr_pypi::PROJECT_DOCUMENT_FORMAT_VERSION;
    }
    Ok(Some(document.to_bytes()))
}
