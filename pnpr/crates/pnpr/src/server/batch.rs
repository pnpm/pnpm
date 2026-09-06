//! `PUT /-/pnpr/v0/publish` — one publish transaction across ecosystems.
//!
//! The npm surface has a batch endpoint of its own (`/-/pnpm/v1/publish`),
//! but its address belongs to that surface: it answers under `/npm/` too. A
//! batch that carries a crate beside a package needs an address that belongs
//! to no ecosystem, so this one lives in pnpr's own namespace, beside
//! `resolve`.
//!
//! Each entry names its `ecosystem` — absent means npm, so an npm batch body
//! is already a valid one — and carries what that surface's own publish
//! endpoint takes, with the binary parts base64-encoded. Every entry is
//! validated and staged before any of them is committed, and the commit is a
//! single journal transaction, so a workspace that spans ecosystems becomes
//! visible all at once.

use super::{
    AppState, AuthedCaller, Identity,
    cargo::{CratePublication, validate_crate_publish},
    publishing::{
        StagedPublish, cleanup_tmp_slots, commit_publishes, publish_created_response,
        stage_publish, validate_publish_doc,
    },
    pypi::{PypiPublication, validate_upload},
};
use axum::{
    body::Bytes,
    extract::State,
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use pnpr_cargo::PublishMetadata;
use pnpr_error::RegistryError;
use pnpr_package_name::PackageName;
use pnpr_pypi::Upload;
use pnpr_registry::Ecosystem;
use pnpr_storage::publish::now_iso;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;

pub(super) async fn serve_ecosystem_publish(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    body: Bytes,
) -> Response {
    match publish_batch(&state, &identity, &body).await {
        Ok(()) => publish_created_response(),
        Err(err) => err.into_response(),
    }
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

/// One entry of the batch, checked as far as it can be before any package
/// lock is held: the caller may publish it, and it carries what it claims.
enum ValidatedEntry {
    Npm(Box<super::publishing::ValidatedPublish>, String),
    Cargo(CratePublication),
    Pypi(PypiPublication),
}

impl ValidatedEntry {
    fn ecosystem(&self) -> Ecosystem {
        match self {
            ValidatedEntry::Npm(..) => Ecosystem::Npm,
            ValidatedEntry::Cargo(_) => Ecosystem::Cargo,
            ValidatedEntry::Pypi(_) => Ecosystem::Pypi,
        }
    }

    fn key(&self) -> &PackageName {
        match self {
            ValidatedEntry::Npm(doc, _) => &doc.name,
            ValidatedEntry::Cargo(publication) => publication.key(),
            ValidatedEntry::Pypi(publication) => publication.key(),
        }
    }

    async fn stage(self, state: &AppState, now: &str) -> Result<StagedPublish, RegistryError> {
        match self {
            ValidatedEntry::Npm(doc, org) => stage_publish(state, *doc, now, Some(&org)).await,
            ValidatedEntry::Cargo(publication) => publication.stage(state).await,
            ValidatedEntry::Pypi(publication) => publication.stage(state).await,
        }
    }
}

async fn publish_batch(
    state: &AppState,
    identity: &Identity,
    body: &Bytes,
) -> Result<(), RegistryError> {
    let mut incoming: Value = serde_json::from_slice(body)?;
    let Some(Value::Array(packages)) =
        incoming.as_object_mut().and_then(|body| body.remove("packages"))
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

    let mut validated = Vec::with_capacity(packages.len());
    let mut seen = HashSet::new();
    for package in packages {
        let entry = validate_entry(state, identity, package).await?;
        // One document read-merge-write per package: the same package twice
        // in a batch would make the second entry's merge depend on the
        // first's uncommitted result. A package of one ecosystem never
        // collides with the same name in another.
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

    // Hold every affected package's lock across the whole stage-and-commit,
    // so concurrent writers of any package in the batch serialize with us
    // just like with a single publish.
    let names: Vec<&str> = validated.iter().map(|entry| entry.key().as_str()).collect();
    let _guards = state.inner.package_locks.lock_many(&names).await;

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
    let outcome = commit_publishes(state, staged).await?;
    if !outcome.unrecorded.is_empty() {
        return Err(RegistryError::PublishNotRecorded { packages: outcome.unrecorded.join(", ") });
    }
    Ok(())
}

/// Check one entry as far as its ecosystem allows before anything is staged:
/// the payload parses, the caller may publish the package, and the bytes are
/// what the entry says they are.
async fn validate_entry(
    state: &AppState,
    identity: &Identity,
    package: Value,
) -> Result<ValidatedEntry, RegistryError> {
    let ecosystem = entry_ecosystem(&package)?;
    match ecosystem {
        Ecosystem::Npm => {
            let name = package.get("name").and_then(Value::as_str).ok_or_else(|| {
                RegistryError::BadRequest {
                    reason: "every npm entry in `packages` must have a string `name`".to_string(),
                }
            })?;
            let name = PackageName::parse(name)?;
            // The batch endpoint is path-less, so each package routes via the
            // default target; validation resolves that route and checks the
            // resolved hosted registry's publish rule per document.
            let (doc, target) = validate_publish_doc(state, identity, None, name, package).await?;
            Ok(ValidatedEntry::Npm(Box::new(doc), target.org))
        }
        Ecosystem::Cargo => {
            let entry: CargoEntry = serde_json::from_value(package)?;
            let archive = decode_base64(&entry.archive, "archive")?;
            let publication =
                validate_crate_publish(state, identity, None, entry.metadata, archive.into())
                    .await?;
            Ok(ValidatedEntry::Cargo(publication))
        }
        Ecosystem::Pypi => {
            let entry: PypiEntry = serde_json::from_value(package)?;
            let upload = Upload {
                name: entry.name,
                version: entry.version,
                filetype: entry.filetype,
                filename: entry.filename,
                content: decode_base64(&entry.content, "content")?,
                sha256_digest: entry.sha256_digest,
                requires_python: entry.requires_python,
            };
            Ok(ValidatedEntry::Pypi(validate_upload(state, identity, None, upload).await?))
        }
    }
}

/// The ecosystem an entry publishes into. An entry that names none is an npm
/// publish document, which is what the npm batch endpoint has always taken.
fn entry_ecosystem(package: &Value) -> Result<Ecosystem, RegistryError> {
    match package.get("ecosystem") {
        None | Some(Value::Null) => Ok(Ecosystem::Npm),
        Some(value) => serde_json::from_value(value.clone()).map_err(|_| {
            RegistryError::BadRequest { reason: format!("unknown ecosystem {value} in `packages`") }
        }),
    }
}

fn decode_base64(data: &str, field: &'static str) -> Result<Vec<u8>, RegistryError> {
    BASE64.decode(data).map_err(|err| RegistryError::BadRequest {
        reason: format!("`{field}` is not valid base64: {err}"),
    })
}
