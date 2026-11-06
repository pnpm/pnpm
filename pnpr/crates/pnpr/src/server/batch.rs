//! `PUT /-/pnpr/v0/publish` — one publish transaction across ecosystems.
//!
//! Mounted at the server root, under `/npm/`, and under a named npm
//! registry's `/npm/~<registry>/`, so both cross-ecosystem releases and
//! `pnpm publish --batch` (which resolves relative to the configured npm
//! registry URL) address the same handler.
//!
//! Each entry names its `ecosystem` — absent means npm, so an npm batch body
//! is already a valid one — and carries what that surface's own publish
//! endpoint takes, with the binary parts base64-encoded. Every entry is
//! validated and staged before any of them is committed, and the whole batch
//! commits as one generation of its organization's catalog, or not at all: a
//! release that spans ecosystems either lands whole or leaves nothing behind,
//! and a reader sees all of it or none of it. A blob whose immutable slot
//! another writer already owns fails the batch before anything is recorded
//! (see `publishing::atomic`).

use super::{
    AppState, AuthedCaller, Identity,
    cargo::{CratePublication, authorize_crate_publish, verify_crate_archive},
    oci::{OciPublication, authorize_publication},
    oidc::WorkloadScope,
    publishing::{
        PublishReceipt, StagedPublish, check_body_versions, cleanup_tmp_slots, commit_publication,
        publication_name, publication_request, publish_committed_response,
        refuse_notes_of_published_python_versions, replayed_outcome, single_catalog, stage_publish,
        take_note, take_version_note, validate_publish_doc,
    },
    pypi::{PypiPublication, authorize_upload, verify_upload},
    resolve_write_target,
    staged_archives::StagedReferences,
};
use axum::{
    Extension,
    body::Bytes,
    extract::{Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use pnpr_cargo::PublishMetadata;
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_pypi::Upload;
use pnpr_registry::Ecosystem;
use pnpr_storage::{
    metadata::{PublicationRequest, PublishNotes},
    publish::now_iso,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;

/// The request header naming a client's idempotency key for a publish.
const IDEMPOTENCY_KEY: &str = "idempotency-key";

pub(super) async fn serve_ecosystem_publish(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    workload: Option<Extension<WorkloadScope>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let caller = BatchCaller { identity: &identity, workload: workload.as_deref(), registry: None };
    serve(&state, caller, &headers, body).await
}

/// `PUT /npm/~<registry>/-/pnpr/v0/publish`: a batch whose npm entries are
/// addressed through one named registry, as a single-package publish to
/// that registry's address is.
pub(super) async fn serve_registry_publish(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    workload: Option<Extension<WorkloadScope>>,
    Path(registry): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let caller = BatchCaller {
        identity: &identity,
        workload: workload.as_deref(),
        registry: Some(&registry),
    };
    serve(&state, caller, &headers, body).await
}

async fn serve(
    state: &AppState,
    caller: BatchCaller<'_>,
    headers: &HeaderMap,
    body: Bytes,
) -> Response {
    let key = headers
        .get(IDEMPOTENCY_KEY)
        .and_then(|key| key.to_str().ok());
    publish_batch(state, caller, key, body).await.unwrap_or_else(IntoResponse::into_response)
}

/// Who sends a batch, and the registry its npm entries are addressed
/// through.
#[derive(Clone, Copy)]
struct BatchCaller<'a> {
    identity: &'a Identity,
    /// Set for workload credentials, which publish only the workload's
    /// packages.
    workload: Option<&'a WorkloadScope>,
    /// The named npm registry the request addresses; `None` routes each
    /// npm entry through the default target.
    registry: Option<&'a str>,
}

/// The `cargo publish` request body, as a batch entry carries it: the same
/// metadata document the native endpoint reads out of its binary framing,
/// and the `.crate` archive base64-encoded.
#[derive(Deserialize)]
struct CargoEntry {
    metadata: PublishMetadata,
    archive: String,
}

/// The legacy-API upload fields a batch entry carries, spelled as the upload
/// form spells them, with the file base64-encoded.
#[derive(Deserialize)]
struct PypiEntry {
    name: String,
    version: String,
    filetype: String,
    filename: String,
    content: String,
    #[serde(default)]
    requires_python: Option<String>,
    #[serde(default)]
    sha256_digest: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OciEntry {
    name: String,
    reference: String,
    manifest: String,
    content_type: Option<String>,
}

/// One entry of the batch, checked as far as it can be before any package
/// lock is held: the caller may publish it, and it carries what it claims.
enum ValidatedEntry {
    Npm(Box<super::publishing::ValidatedPublish>, String),
    Cargo(CratePublication),
    Pypi(PypiPublication),
    Oci(OciPublication),
}

impl ValidatedEntry {
    fn ecosystem(&self) -> Ecosystem {
        match self {
            ValidatedEntry::Npm(..) => Ecosystem::Npm,
            ValidatedEntry::Cargo(_) => Ecosystem::Cargo,
            ValidatedEntry::Pypi(_) => Ecosystem::Pypi,
            ValidatedEntry::Oci(_) => Ecosystem::Oci,
        }
    }

    /// The organization whose catalog the entry is published into.
    fn org(&self) -> &str {
        match self {
            ValidatedEntry::Npm(_, org) => org,
            ValidatedEntry::Cargo(publication) => publication.org(),
            ValidatedEntry::Pypi(publication) => publication.org(),
            ValidatedEntry::Oci(publication) => publication.org(),
        }
    }

    fn key(&self) -> &CanonicalPackageName {
        match self {
            ValidatedEntry::Npm(doc, _) => &doc.name,
            ValidatedEntry::Cargo(publication) => publication.key(),
            ValidatedEntry::Pypi(publication) => publication.key(),
            ValidatedEntry::Oci(publication) => publication.key(),
        }
    }

    async fn stage(self, state: &AppState, now: &str) -> Result<StagedPublish, RegistryError> {
        match self {
            ValidatedEntry::Npm(doc, org) => stage_publish(state, *doc, now, Some(&org)).await,
            ValidatedEntry::Cargo(publication) => publication.stage(state).await,
            ValidatedEntry::Pypi(publication) => publication.stage(state).await,
            ValidatedEntry::Oci(publication) => publication.stage(state).await.map_err(Into::into),
        }
    }
}

async fn publish_batch(
    state: &AppState,
    caller: BatchCaller<'_>,
    key: Option<&str>,
    body: Bytes,
) -> Result<Response, RegistryError> {
    let identity = caller.identity;
    let mut incoming: Value = serde_json::from_slice(&body).map_err(|err| malformed_body(&err))?;
    // The parsed body holds everything the entries need, and the payloads are
    // decoded out of it below: keep only one copy of a body that may run to
    // the request limit.
    drop(body);
    let name = publication_name(&incoming)?;
    let request = publication_request(identity, &incoming, key, name, None);
    let mut packages = take_packages(&mut incoming)?;
    let notes = batch_notes(&mut incoming, &mut packages)?;
    let validated = validate_batch_entries(state, caller, packages).await?;

    // Hold every affected package's lock across the whole stage-and-commit,
    // so concurrent writers of any package in the batch serialize with us
    // just like with a single publish.
    let names: Vec<&str> = validated
        .iter()
        .map(|entry| entry.key().as_str())
        .collect();
    let _guards = state.inner.locks.packages.lock_many(&names).await;
    // Every entry is authorized above, so a retry gets the outcome of the
    // request it repeats only while its caller may still make it. It is looked
    // up under the locks, so a retry that waited for the original sees its
    // commit.
    if let Some(replayed) = replayed_batch(state, request.as_ref(), &validated).await? {
        return Ok(replayed);
    }
    refuse_late_python_notes(state, &validated, &notes).await?;

    let receipt = stage_and_commit(state, identity, validated, notes, request).await?;
    Ok(publish_committed_response(&receipt))
}

/// Stage every entry and commit them as one publication, then give back the
/// staged archives the npm entries claimed.
async fn stage_and_commit(
    state: &AppState,
    identity: &Identity,
    validated: Vec<ValidatedEntry>,
    notes: PublishNotes,
    request: Option<PublicationRequest>,
) -> Result<PublishReceipt, RegistryError> {
    let storage = &state.inner.storage;
    let mut integrities: Vec<&str> = Vec::new();
    for entry in &validated {
        if let ValidatedEntry::Npm(doc, _) = entry {
            integrities.extend(doc.staged_integrities());
        }
    }
    let references = StagedReferences::check(storage, identity, integrities).await?;
    let staged = stage_entries(state, validated).await?;
    let receipt = commit_publication(state, staged, notes, request).await?;
    references.release(storage).await;
    Ok(receipt)
}

/// Refuse the batch's notes for Python versions already published, in the
/// one catalog its entries target.
async fn refuse_late_python_notes(
    state: &AppState,
    validated: &[ValidatedEntry],
    notes: &PublishNotes,
) -> Result<(), RegistryError> {
    if notes.versions.is_empty() {
        return Ok(());
    }
    let catalog = single_catalog(validated.iter().map(ValidatedEntry::org))?;
    refuse_notes_of_published_python_versions(state, &catalog, notes).await
}

/// The recorded outcome of the batch `request` repeats, in the one catalog
/// its entries target. A request exists only with `SQLite` metadata, whose
/// publish targets one catalog.
async fn replayed_batch(
    state: &AppState,
    request: Option<&PublicationRequest>,
    validated: &[ValidatedEntry],
) -> Result<Option<Response>, RegistryError> {
    if request.is_none() {
        return Ok(None);
    }
    let catalog = single_catalog(validated.iter().map(ValidatedEntry::org))?;
    replayed_outcome(state, &catalog, request).await
}

/// Stage every entry, removing what was staged when one fails.
async fn stage_entries(
    state: &AppState,
    validated: Vec<ValidatedEntry>,
) -> Result<Vec<StagedPublish>, RegistryError> {
    let now = now_iso();
    let mut staged: Vec<StagedPublish> = Vec::with_capacity(validated.len());
    for entry in validated {
        match entry.stage(state, &now).await {
            Ok(stage) => staged.push(stage),
            Err(err) => {
                for stage in staged {
                    cleanup_tmp_slots(stage.slots).await;
                }
                return Err(err);
            }
        }
    }
    Ok(staged)
}

/// The publication note at the top of a batch body and the version note of
/// each entry, taken out of the body.
fn batch_notes(
    incoming: &mut Value,
    packages: &mut [Value],
) -> Result<PublishNotes, RegistryError> {
    let publication = take_note(incoming)?;
    let mut versions = Vec::new();
    for package in packages {
        let ecosystem = entry_ecosystem(package)?;
        versions.extend(take_version_note(package, ecosystem)?);
    }
    Ok(PublishNotes { publication, versions })
}

/// The `packages` array of a batch body, which must not be empty.
fn take_packages(incoming: &mut Value) -> Result<Vec<Value>, RegistryError> {
    let Some(Value::Array(packages)) = incoming
        .as_object_mut()
        .and_then(|body| body.remove("packages"))
    else {
        return Err(RegistryError::BadRequest {
            reason: "body must be an object with a `packages` array".to_string(),
        });
    };
    if packages.is_empty() {
        return Err(RegistryError::BadRequest {
            reason: "`packages` must not be empty".to_string(),
        });
    }
    Ok(packages)
}

/// Validate every entry of a batch, refusing a package named twice.
///
/// One document read-merge-write happens per package: the same package twice
/// in a batch would make the second entry's merge depend on the first's
/// uncommitted result. A package of one ecosystem never collides with the same
/// name in another.
async fn validate_batch_entries(
    state: &AppState,
    caller: BatchCaller<'_>,
    packages: Vec<Value>,
) -> Result<Vec<ValidatedEntry>, RegistryError> {
    let mut validated = Vec::with_capacity(packages.len());
    let mut seen = HashSet::new();
    for package in packages {
        let entry = validate_entry(state, caller, package).await?;
        if !seen.insert((entry.ecosystem(), entry.key().as_str().to_string())) {
            return Err(RegistryError::BadRequest {
                reason: format!(
                    "duplicate {} package {:?} in `packages`",
                    entry.ecosystem(),
                    entry.key().as_str(),
                ),
            });
        }
        validated.push(entry);
    }
    Ok(validated)
}

/// Check one entry as far as its ecosystem allows before anything is staged:
/// the payload parses, the caller may publish the package, and the bytes are
/// what the entry says they are.
async fn validate_entry(
    state: &AppState,
    caller: BatchCaller<'_>,
    package: Value,
) -> Result<ValidatedEntry, RegistryError> {
    let ecosystem = entry_ecosystem(&package)?;
    if let Some(workload) = caller.workload {
        workload.check_ecosystem(ecosystem)?;
    }
    let identity = caller.identity;
    match ecosystem {
        Ecosystem::Npm => validate_npm_entry(state, caller, package).await,
        Ecosystem::Cargo => {
            let entry: CargoEntry =
                serde_json::from_value(package).map_err(|err| malformed_body(&err))?;
            // Decide where this crate writes, and whether the caller may write
            // it, before spending anything on decoding its archive.
            let target = authorize_crate_publish(state, identity, None, &entry.metadata)?;
            let archive = decode_base64(&entry.archive, "archive")?;
            Ok(ValidatedEntry::Cargo(
                verify_crate_archive(target, entry.metadata, archive.into()).await?,
            ))
        }
        Ecosystem::Pypi => validate_pypi_entry(state, identity, package).await,
        Ecosystem::Oci => {
            let entry: OciEntry =
                serde_json::from_value(package).map_err(|err| malformed_body(&err))?;
            let target = authorize_publication(state, identity, None, &entry.name)
                .map_err(RegistryError::from)?;
            let bytes = decode_base64(&entry.manifest, "manifest")?;
            OciPublication::new(
                target,
                entry.reference,
                bytes.into(),
                entry.content_type.as_deref(),
                state.inner.config.http.oci.max_manifest_bytes,
            )
            .map(ValidatedEntry::Oci)
            .map_err(RegistryError::from)
        }
    }
}

/// The ecosystem an entry publishes into. An entry that names none is an npm
/// publish document, which is what the npm batch endpoint has always taken.
fn entry_ecosystem(package: &Value) -> Result<Ecosystem, RegistryError> {
    match package.get("ecosystem") {
        None | Some(Value::Null) => Ok(Ecosystem::Npm),
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|_| RegistryError::BadRequest {
                reason: format!("unknown ecosystem {value} in `packages`"),
            }),
    }
}

/// A body this endpoint could not read is the client's mistake, not a
/// gateway's: `RegistryError::Json` answers 502, which is for the JSON an
/// upstream sent us.
fn malformed_body(err: &serde_json::Error) -> RegistryError {
    RegistryError::BadRequest { reason: format!("malformed publish batch: {err}") }
}

fn decode_base64(data: &str, field: &'static str) -> Result<Vec<u8>, RegistryError> {
    BASE64
        .decode(data)
        .map_err(|err| RegistryError::BadRequest {
            reason: format!("`{field}` is not valid base64: {err}"),
        })
}

/// Remove the batch routing field before the npm merge can persist it in the
/// packument, and authorize against the hosted registry the entry resolves
/// to through the addressed registry, or the default target.
async fn validate_npm_entry(
    state: &AppState,
    caller: BatchCaller<'_>,
    package: Value,
) -> Result<ValidatedEntry, RegistryError> {
    let mut package = package;
    if let Some(entry) = package.as_object_mut() {
        entry.remove("ecosystem");
    }
    check_body_versions(&package)?;
    let name = package
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RegistryError::BadRequest {
            reason: "every npm entry in `packages` must have a string `name`".to_string(),
        })?;
    let name = CanonicalPackageName::parse(name, pnpr_package_name::Ecosystem::Npm)?;
    // A workload's entry is held to its scope before anything else is read
    // for it, by the route the publish resolves to.
    if let Some(workload) = caller.workload {
        let target = resolve_write_target(state, caller.identity, caller.registry, &name)?;
        workload.check_npm_publish(&target.source, name.as_str())?;
    }
    // Validation resolves the route and checks the resolved hosted
    // registry's publish rule per document.
    let (doc, target) =
        validate_publish_doc(state, caller.identity, caller.registry, name, package).await?;
    Ok(ValidatedEntry::Npm(Box::new(doc), target.org))
}

async fn validate_pypi_entry(
    state: &AppState,
    identity: &Identity,
    package: Value,
) -> Result<ValidatedEntry, RegistryError> {
    let entry: PypiEntry = serde_json::from_value(package).map_err(|err| malformed_body(&err))?;
    let mut upload = Upload {
        name: entry.name,
        version: entry.version,
        filetype: entry.filetype,
        filename: entry.filename,
        content: Vec::new(),
        sha256_digest: entry.sha256_digest,
        requires_python: entry.requires_python,
    };
    let target = authorize_upload(state, identity, None, &upload)?;
    upload.content = decode_base64(&entry.content, "content")?;
    Ok(ValidatedEntry::Pypi(verify_upload(target, upload).await?))
}
