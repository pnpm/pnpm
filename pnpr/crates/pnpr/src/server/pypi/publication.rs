use super::{
    Action, AppState, AuthedCaller, BTreeMap, Bytes, CanonicalPackageName, DistributionKind,
    ECOSYSTEM, HeaderMap, Identity, ProjectDocument, ProjectFile, PublishTarget, RegistryError,
    RegistrySource, Request, Response, StagedPublish, State, StatusCode, TargetRegistry, Upload,
    Yanked, authorize, bad_request, header, multipart, normalize_version, now_iso,
    parse_distribution_filename, parse_upload, private_no_cache, resolve_publish_target_for,
    sha256_hex, stage_hosted_artifact, store_hosted_artifact,
};
use axum::{extract::FromRequest as _, response::IntoResponse};

/// `POST legacy/` — the legacy upload API `twine` speaks.
pub(super) async fn post_upload(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    headers: HeaderMap,
    request: Request,
) -> Response {
    if matches!(identity, Identity::Anonymous) {
        return private_no_cache(
            RegistryError::Unauthenticated { resource: "Python uploads".to_string() }
                .into_response(),
        );
    }
    let body = match Bytes::from_request(request, &state).await {
        Ok(body) => body,
        Err(err) => return private_no_cache(err.into_response()),
    };
    let response = match upload_file(&state, &identity, registry.as_deref(), &headers, &body).await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(err) => err.into_response(),
    };
    private_no_cache(response)
}

pub(super) async fn upload_file(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<(), RegistryError> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| bad_request("request body must be multipart/form-data"))?;
    let parts = multipart::parse_form(content_type, body).map_err(bad_request)?;
    let upload = parse_upload(parts).map_err(bad_request)?;
    validate_upload(state, identity, registry, upload).await?.publish(state).await
}

/// An upload that may proceed: the caller is allowed to publish the project,
/// the file's name matches the project and version it claims, and the entry
/// that will record it is built. Publishing it is [`Self::publish`] on its
/// own, or [`Self::stage`] as one package of a cross-ecosystem batch.
pub(in super::super) struct PypiPublication {
    pub(super) key: CanonicalPackageName,
    pub(super) org: String,
    pub(super) entry: ProjectFile,
    pub(super) content: Vec<u8>,
}

/// Every check a legacy-API upload must pass before anything is written.
pub(in super::super) async fn validate_upload(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    upload: Upload,
) -> Result<PypiPublication, RegistryError> {
    let target = authorize_upload(state, identity, registry, &upload)?;
    verify_upload(target, upload)
}

/// Where an upload writes, once its fields are consistent and the caller is
/// allowed to publish the project there. Everything this decides is cheap, so
/// a caller holding an undecoded payload can settle the question before
/// spending anything on the file.
pub(in super::super) struct PypiTarget {
    pub(super) key: CanonicalPackageName,
    pub(super) org: String,
}

pub(in super::super) fn authorize_upload(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    upload: &Upload,
) -> Result<PypiTarget, RegistryError> {
    let key = CanonicalPackageName::parse(&upload.name, ECOSYSTEM)?;
    let project = key.as_str();
    let version = normalize_version(&upload.version).map_err(bad_request)?;
    let distribution = parse_distribution_filename(&upload.filename).map_err(bad_request)?;
    if distribution.name != project {
        return Err(bad_request(format!(
            "filename {:?} does not belong to project {project:?}",
            upload.filename,
        )));
    }
    if normalize_version(&distribution.version).map_err(bad_request)? != version {
        return Err(bad_request(format!(
            "filename {:?} does not carry version {version:?}",
            upload.filename,
        )));
    }
    match (upload.filetype.as_str(), distribution.kind) {
        ("bdist_wheel", DistributionKind::Wheel) | ("sdist", DistributionKind::Sdist) => {}
        (filetype, _) => {
            return Err(bad_request(format!(
                "filetype {filetype:?} does not match the filename {:?}",
                upload.filename,
            )));
        }
    }
    let (source, org) =
        match resolve_publish_target_for(state, identity, registry, ECOSYSTEM, project) {
            PublishTarget::Hosted { source, org } => (source, org),
            PublishTarget::Reject(reason) => return Err(RegistryError::BadRequest { reason }),
            PublishTarget::Denied(err) => return Err(err),
            PublishTarget::NotFound => return Err(RegistryError::NotFound),
        };
    authorize(state, identity, &RegistrySource::Hosted(source), project, Action::Publish)?;
    Ok(PypiTarget { key, org })
}

/// Check the file against the digest it was uploaded with, and build the
/// entry that will record it.
pub(in super::super) fn verify_upload(
    target: PypiTarget,
    upload: Upload,
) -> Result<PypiPublication, RegistryError> {
    let PypiTarget { key, org } = target;
    let sha256 = sha256_hex(&upload.content);
    if upload
        .sha256_digest
        .as_deref()
        .is_some_and(|declared| !declared.eq_ignore_ascii_case(&sha256))
    {
        return Err(bad_request("sha256_digest does not match the uploaded file"));
    }
    let entry = ProjectFile {
        filename: upload.filename,
        url: None,
        hashes: BTreeMap::from([("sha256".to_string(), sha256)]),
        requires_python: upload.requires_python,
        yanked: Yanked::Flag(false),
        size: Some(upload.content.len() as u64),
        upload_time: Some(now_iso()),
    };
    Ok(PypiPublication { key, org, entry, content: upload.content })
}

impl PypiPublication {
    pub(in super::super) fn key(&self) -> &CanonicalPackageName {
        &self.key
    }

    /// Publish this file on its own, in a transaction of one.
    pub(super) async fn publish(self, state: &AppState) -> Result<(), RegistryError> {
        store_hosted_artifact(
            state,
            &self.org,
            &self.key,
            &self.entry.filename.clone(),
            &self.content,
            refuse_existing_file(&self.entry.filename),
            ProjectDocument {
                name: self.key.as_str().to_string(),
                files: vec![self.entry.clone()],
            },
        )
        .await
    }

    /// Stage this file as one package of a larger transaction. The caller
    /// holds the package lock and commits.
    pub(in super::super) async fn stage(
        self,
        state: &AppState,
    ) -> Result<StagedPublish, RegistryError> {
        stage_hosted_artifact(
            state,
            &self.org,
            &self.key,
            &self.entry.filename.clone(),
            &self.content,
            &refuse_existing_file(&self.entry.filename),
            ProjectDocument {
                name: self.key.as_str().to_string(),
                files: vec![self.entry.clone()],
            },
        )
        .await
    }
}

/// A published distribution file is immutable, so a document that already
/// carries `filename` is one this upload must not land on.
pub(super) fn refuse_existing_file(
    filename: &str,
) -> impl Fn(&ProjectDocument) -> Result<(), RegistryError> {
    let filename = filename.to_string();
    move |document: &ProjectDocument| match document.file(&filename) {
        Some(_) => {
            Err(RegistryError::BadRequest { reason: format!("File already exists: {filename:?}") })
        }
        None => Ok(()),
    }
}
