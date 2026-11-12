use super::{
<<<<<<< HEAD
    super::{Ownership, authorize_with_ownership, require_admin, resolve_write_target_for},
    Action, AppState, AuthedCaller, CanonicalPackageName, ECOSYSTEM, HashMap, Identity, Path,
    ProjectDocument, RegistryError, RegistrySource, Response, State, StatusCode, TargetRegistry,
    Yanked, is_safe_path_segment, not_found, private_no_cache,
=======
    super::{require_admin, resolve_write_target_for},
    Action, AppState, AuthedCaller, CanonicalPackageName, ECOSYSTEM, HashMap, Identity, Ownership,
    Path, ProjectDocument, RegistryError, RegistrySource, Response, State, StatusCode,
    TargetRegistry, Yanked, authorize_package, authorize_with_ownership, is_safe_path_segment,
    not_found, private_no_cache,
>>>>>>> 918dce071c4 (fix(pnpr): bind package mutation and publish authorization to storage transactions)
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
    let _guard = state.inner.locks.packages.lock(key.as_str()).await;
    let hosted = RegistrySource::Hosted(target.source);
    let mut authorized = yanked.is_none();
    let outcome = state.inner.storage
        .for_hosted(&target.namespace, ECOSYSTEM)
        .update_hosted_document_with_retry(&key, DOCUMENT_WRITE_RETRIES, None, |existing| {
            let Some(bytes) = existing else { return Ok(None) };
            let document = ProjectDocument::parse(bytes)?;
            if yanked.is_some() {
                authorize_project_yank(state, identity, &hosted, (&key, &document))?;
                authorized = true;
            }
            update_document(document, &key, filename, yanked.as_ref())
        })
        .await?;
    if !authorized {
        authorize_package(
            state,
            identity,
            &hosted,
            key.as_str(),
            Action::Publish,
            Some(target.namespace.as_str()),
        )
        .await?;
    }
    Ok(outcome)
}

fn authorize_project_yank(
    state: &AppState,
    identity: &Identity,
    hosted: &RegistrySource,
    (key, document): (&CanonicalPackageName, &ProjectDocument),
) -> Result<(), RegistryError> {
    let ownership = Ownership::Owners(&document.owners);
    authorize_with_ownership(state, identity, hosted, (key.as_str(), &ownership), Action::Publish)
}

fn update_document(
    mut document: ProjectDocument,
    key: &CanonicalPackageName,
    filename: &str,
    yanked: Option<&Yanked>,
) -> Result<Option<Vec<u8>>, RegistryError> {
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
