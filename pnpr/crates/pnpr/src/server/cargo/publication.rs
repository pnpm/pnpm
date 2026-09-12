use super::{
    Action, AppState, AuthedCaller, Bytes, CanonicalPackageName, CrateDocument,
    DOCUMENT_WRITE_RETRIES, DocumentUpdate, ECOSYSTEM, HashMap, Identity, IndexEntry, Path,
    PublishMetadata, PublishTarget, RegistryError, RegistrySource, Response, StagedPublish, State,
    StatusCode, TargetRegistry, authorize, bad_request, bounded_description, crate_filename,
    error_response, json_response, not_found, ok_json, parse_publish_body, publish_ok_json,
    resolve_publish_target_for, resolve_write_target_for, sha256_hex, stage_hosted_artifact,
    store_hosted_artifact, validate_crate_archive,
};

/// `PUT api/v1/crates/new` — `cargo publish`.
pub(super) async fn put_publish(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    body: Bytes,
) -> Response {
    let (metadata, archive) = match parse_publish_body(&body) {
        Ok(parsed) => parsed,
        Err(err) => return bad_request(err),
    };
    // The archive is the tail of the body; re-slice it so the checks below
    // can own it without copying.
    let archive = body.slice(body.len() - archive.len()..);
    let publication =
        match validate_crate_publish(&state, &identity, registry.as_deref(), metadata, archive)
            .await
        {
            Ok(publication) => publication,
            Err(err) => return error_response(err),
        };
    match publication.publish(&state).await {
        Ok(()) => json_response(StatusCode::OK, &publish_ok_json()),
        Err(err) => error_response(err),
    }
}

/// A `cargo publish` that may proceed: the caller is allowed to publish the
/// crate, the archive holds what its metadata says, and the index entry that
/// will record it is built. Publishing it is [`Self::publish`] on its own, or
/// [`Self::stage`] as one package of a cross-ecosystem batch.
pub(in super::super) struct CratePublication {
    pub(super) key: CanonicalPackageName,
    pub(super) org: String,
    pub(super) filename: String,
    pub(super) entry: IndexEntry,
    pub(super) description: Option<String>,
    pub(super) archive: Bytes,
}

/// Every check a `cargo publish` must pass before anything is written.
pub(in super::super) async fn validate_crate_publish(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    metadata: PublishMetadata,
    archive: Bytes,
) -> Result<CratePublication, RegistryError> {
    let target = authorize_crate_publish(state, identity, registry, &metadata)?;
    verify_crate_archive(target, metadata, archive).await
}

/// Where a crate publish writes, once the metadata is well-formed and the
/// caller is allowed to publish it there. Everything this decides is cheap,
/// so a caller holding an undecoded payload can settle the question before
/// spending anything on the archive.
pub(in super::super) struct CrateTarget {
    pub(super) key: CanonicalPackageName,
    pub(super) org: String,
}

pub(in super::super) fn authorize_crate_publish(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    metadata: &PublishMetadata,
) -> Result<CrateTarget, RegistryError> {
    metadata.validate().map_err(|err| RegistryError::BadRequest { reason: err.to_string() })?;
    let key = CanonicalPackageName::parse(&metadata.name, ECOSYSTEM)?;
    let (source, org) =
        match resolve_publish_target_for(state, identity, registry, ECOSYSTEM, key.as_str()) {
            PublishTarget::Hosted { source, org } => (source, org),
            PublishTarget::Reject(reason) => return Err(RegistryError::BadRequest { reason }),
            PublishTarget::Denied(err) => return Err(err),
            PublishTarget::NotFound => return Err(RegistryError::NotFound),
        };
    authorize(state, identity, &RegistrySource::Hosted(source), key.as_str(), Action::Publish)?;
    Ok(CrateTarget { key, org })
}

/// Check the archive against the metadata it was published with, and build
/// the index entry that will record it.
pub(in super::super) async fn verify_crate_archive(
    target: CrateTarget,
    metadata: PublishMetadata,
    archive: Bytes,
) -> Result<CratePublication, RegistryError> {
    let CrateTarget { key, org } = target;
    let (name, version) = (metadata.name.clone(), metadata.vers.clone());
    let checked = tokio::task::spawn_blocking(move || {
        validate_crate_archive(&archive, &name, &version)
            .map(|()| (sha256_hex(&archive), archive))
            .map_err(|err| RegistryError::BadRequest { reason: err.to_string() })
    })
    .await
    .map_err(RegistryError::JoinError)??;
    let (cksum, archive) = checked;
    let filename = crate_filename(&metadata.name, &metadata.vers);
    let description = bounded_description(metadata.description.as_deref());
    Ok(CratePublication {
        key,
        org,
        filename,
        entry: metadata.into_index_entry(cksum),
        description,
        archive,
    })
}

impl CratePublication {
    pub(in super::super) fn key(&self) -> &CanonicalPackageName {
        &self.key
    }

    /// The one-version document this publish contributes, which merges into
    /// whatever the registry already holds for the crate.
    pub(super) fn document(&self) -> CrateDocument {
        CrateDocument {
            name: self.entry.name.clone(),
            versions: vec![self.entry.clone()],
            description: self.description.clone(),
        }
    }

    /// Publish this crate on its own, in a transaction of one.
    pub(super) async fn publish(self, state: &AppState) -> Result<(), RegistryError> {
        store_hosted_artifact(
            state,
            &self.org,
            &self.key,
            &self.filename,
            &self.archive,
            refuse_published_version(&self.entry.vers),
            self.document(),
        )
        .await
    }

    /// Stage this crate as one package of a larger transaction. The caller
    /// holds the package lock and commits.
    pub(in super::super) async fn stage(
        self,
        state: &AppState,
    ) -> Result<StagedPublish, RegistryError> {
        stage_hosted_artifact(
            state,
            &self.org,
            &self.key,
            &self.filename,
            &self.archive,
            &refuse_published_version(&self.entry.vers),
            self.document(),
        )
        .await
    }
}

/// A published crate version is immutable, so a document that already carries
/// `vers` is one this publish must not land on.
pub(super) fn refuse_published_version(
    vers: &str,
) -> impl Fn(&CrateDocument) -> Result<(), RegistryError> {
    let vers = vers.to_string();
    move |document: &CrateDocument| match document.version(&vers) {
        Some(_) => Err(RegistryError::BadRequest {
            reason: format!("crate version `{vers}` is already uploaded"),
        }),
        None => Ok(()),
    }
}

/// `DELETE api/v1/crates/<crate>/<version>/yank`.
pub(super) async fn delete_yank(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(params): Path<HashMap<String, String>>,
) -> Response {
    set_yanked(&state, &identity, registry.as_deref(), &params, true).await
}

/// `PUT api/v1/crates/<crate>/<version>/unyank`.
pub(super) async fn put_unyank(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(params): Path<HashMap<String, String>>,
) -> Response {
    set_yanked(&state, &identity, registry.as_deref(), &params, false).await
}

/// Yanking is an owner action on crates.io, so it takes the same `publish`
/// permission a new version does.
pub(super) async fn set_yanked(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    params: &HashMap<String, String>,
    yanked: bool,
) -> Response {
    let (Some(name), Some(version)) = (params.get("name"), params.get("version")) else {
        return not_found();
    };
    let key = match CanonicalPackageName::parse(name, ECOSYSTEM) {
        Ok(key) => key,
        Err(err) => return error_response(err),
    };
    let target = match resolve_write_target_for(state, identity, registry, ECOSYSTEM, &key) {
        Ok(target) => target,
        Err(err) => return error_response(err),
    };
    if let Err(err) = authorize(
        state,
        identity,
        &RegistrySource::Hosted(target.source),
        key.as_str(),
        Action::Publish,
    ) {
        return error_response(err);
    }
    let _guard = state.inner.package_locks.lock(key.as_str()).await;
    let outcome = state
        .inner
        .storage
        .for_hosted(&target.org)
        .update_hosted_document_with_retry(&key, DOCUMENT_WRITE_RETRIES, |existing| {
            let Some(bytes) = existing else { return Ok(None) };
            let mut document = CrateDocument::parse(bytes)?;
            let Some(entry) = document.version_mut(version) else { return Ok(None) };
            entry.yanked = yanked;
            Ok(Some(document.to_bytes()))
        })
        .await;
    match outcome {
        Ok(DocumentUpdate::Written) => json_response(StatusCode::OK, &ok_json()),
        Ok(DocumentUpdate::NotFound) => not_found(),
        Err(err) => error_response(err),
    }
}
