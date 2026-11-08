//! The transparency log of a hosted registry's release records,
//! `<base>/-/log/`, and a record's evidence,
//! `<base>/<name>/<version>+r<revision>/evidence`
//! ([release records](https://github.com/pnpm/rfcs/blob/92d2b281af321ecf8f1e5fea63f698716ba4e9e7/pnpr/text/0000-release-records.md#client-protocol)).
//!
//! A registry address serves the log of the one hosted organization it
//! reaches. Its checkpoint and hash tiles are read by anyone who may read a
//! package of that organization, for consistency checks. Its entry bundles
//! name every logged package, so only a caller who may read every package
//! the log holds gets them; anyone else gets the same not-found for every
//! bundle, whatever it holds. A record's reader gets the record's own entry,
//! by its index, with its inclusion proof, in the record's evidence, beside
//! the registry's statements about the record.

use super::{
    AppState, AuthedCaller, Ecosystem, Identity, RegistrySource, Response, StatusCode,
    TargetRegistry, addressed_registry, caller_scoped, hosted_read_namespace, not_found,
    private_no_cache, resolve_registry_source,
};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderValue, header},
    response::IntoResponse,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use pnpm_release_record::{AcceptancePredicate, Envelope, PredicateType, Statement, StoredRecord};
use pnpm_transparency_log::TilePath;
use pnpr_config::HostedConfig;
use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::transparency_log::{InclusionProof, LogId};
use serde_json::json;
use std::time::Duration;

/// How long a read of a record's evidence waits for the record's entry to
/// be integrated before it answers that the proof is pending.
const PROOF_WAIT: Duration = Duration::from_secs(5);

/// The path of a log resource, beside the addressed registry's name.
#[derive(serde::Deserialize)]
pub(super) struct LogPath {
    resource: String,
}

/// `GET <base>/-/log/{*resource}`: the log's `checkpoint`, a hash tile
/// `tile/<L>/<N>[.p/<W>]`, or an entry bundle `tile/entries/<N>[.p/<W>]`.
pub(super) async fn get_log_resource(
    State(state): State<AppState>,
    AuthedCaller(identity): AuthedCaller,
    TargetRegistry(registry): TargetRegistry,
    Path(LogPath { resource }): Path<LogPath>,
) -> Response {
    let Some(log) = registry_log(&state, registry.as_deref()) else { return not_found() };
    // A server whose router was built outside a runtime recovers its logs
    // at its first read of one.
    super::release_record_logs::integrate_sequenced_logs(&state);
    let readers = LogReaders::of(&state, &log, &identity);
    let anyone = LogReaders::of(&state, &log, &Identity::Anonymous);
    if !readers.may_read_some {
        return not_found();
    }
    let (response, public) = if resource == "checkpoint" {
        (serve_checkpoint(&state, &log).await, anyone.may_read_some)
    } else {
        match resource.parse::<TilePath>() {
            Ok(TilePath::Entries { .. }) if !readers.may_read_all => (not_found(), false),
            Ok(path @ TilePath::Entries { .. }) => {
                (serve_tile(&state, &log, path).await, anyone.may_read_all)
            }
            Ok(path) => (serve_tile(&state, &log, path).await, anyone.may_read_some),
            Err(_) => (not_found(), false),
        }
    };
    // A named registry's responses are always caller-scoped.
    if public && registry.is_none() { response } else { private_no_cache(response) }
}

async fn serve_checkpoint(state: &AppState, log: &LogId) -> Response {
    match state.inner.storage.log_checkpoint(log).await {
        Ok(Some(checkpoint)) => {
            let mut response = bytes_response(checkpoint.note, "text/plain; charset=utf-8");
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
            response
        }
        Ok(None) => not_found(),
        Err(err) => err.into_response(),
    }
}

/// A tile never changes once written, under its full or partial path.
async fn serve_tile(state: &AppState, log: &LogId, path: TilePath) -> Response {
    match state.inner.storage.log_tile(log, path).await {
        Ok(Some(tile)) => {
            let mut response = bytes_response(tile, "application/octet-stream");
            response
                .headers_mut()
                .insert(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("public, max-age=31536000, immutable"),
                );
            response
        }
        Ok(None) => not_found(),
        Err(err) => err.into_response(),
    }
}

fn bytes_response(body: Vec<u8>, content_type: &'static str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .expect("static-shape response always builds")
}

/// The log a registry address serves: the npm log of the one hosted
/// organization the addressed registry reaches. A registry that reaches
/// none, or several, has no log.
fn registry_log(state: &AppState, registry: Option<&str>) -> Option<LogId> {
    let target = addressed_registry(state, registry, Ecosystem::Npm)?;
    let routing = &state.inner.config.routing;
    let mut orgs = routing.registries
        .sources(&target, Ecosystem::Npm)
        .into_iter()
        .filter_map(|source| routing.hosted.get(source))
        .map(|hosted| hosted.org.as_str());
    let org = orgs.next()?;
    if orgs.any(|other| other != org) {
        return None;
    }
    state.inner.config.release_records.log_signing_key.as_ref()?;
    Some(LogId { catalog: org.to_string(), ecosystem: Ecosystem::Npm })
}

/// What one caller may read of a log.
struct LogReaders {
    /// The caller may read some package the log holds.
    may_read_some: bool,
    /// The caller may read every package the log holds.
    may_read_all: bool,
}

impl LogReaders {
    /// What `identity` may read of `log`, by the rules of every hosted
    /// registry of its organization.
    fn of(state: &AppState, log: &LogId, identity: &Identity) -> Self {
        let hosted: Vec<&HostedConfig> = state.inner.config.routing.hosted
            .iter()
            .filter(|(name, hosted)| {
                hosted.org == log.catalog
                    && state.inner.config.routing.registries.ecosystem(name) == Some(log.ecosystem)
            })
            .map(|(_, hosted)| hosted)
            .collect();
        let is_admin = identity
            .user_id()
            .is_some_and(|username| state.inner.config.identity.admins.contains(username));
        // No registry's rules admit anyone to a log no registry holds.
        let may_read_all = is_admin
            || (!hosted.is_empty()
                && hosted
                    .iter()
                    .all(|hosted| hosted.rules.snapshot().all_access_admit(identity)));
        let may_read_some = may_read_all
            || hosted
                .iter()
                .any(|hosted| hosted.rules.snapshot().any_access_admits(identity));
        Self { may_read_some, may_read_all }
    }
}

/// `GET <base>/<name>/<version>+r<revision>/evidence`: the record's digest
/// and the inclusion proof of its log entry, with the registry's statements
/// and the publisher's Sigstore bundle. It is read as the record is, and changes with every newer
/// checkpoint, so it is revalidated on every use.
pub(super) async fn serve_record_evidence(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    address: &str,
) -> Response {
    let response = record_evidence(state, identity, registry, raw_name, address).await;
    caller_scoped(state, Ecosystem::Npm, registry, Some(raw_name), response)
}

async fn record_evidence(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    address: &str,
) -> Response {
    let logged = match logged_record(state, identity, registry, raw_name, address).await {
        Ok(logged) => logged,
        Err(response) => return response,
    };
    let log = LogId { catalog: logged.org.clone(), ecosystem: Ecosystem::Npm };
    let inclusion = match wait_for_inclusion(state, &log, logged.index).await {
        Ok(Some(inclusion)) => inclusion,
        Ok(None) => return proof_pending(),
        Err(err) => return err.into_response(),
    };
    let statements = match registry_statements(state, &logged).await {
        Ok(Some(statements)) => statements,
        // The record's log entry was read, and the record was not: either
        // its version was removed meanwhile, or a replica has not caught up
        // yet. A retry reads both again and answers for what holds then.
        Ok(None) => return retry_later("the record's metadata is changing; retry"),
        Err(err) => return err.into_response(),
    };
    match sigstore_bundle(state, &logged.org, &logged.name, &logged.version).await {
        Ok(sigstore) => evidence_response(&logged.digest, &inclusion, &statements, sigstore),
        Err(err) => err.into_response(),
    }
}

/// The statements the registry signs about a logged record: that it
/// accepted the record under its profile. Each is signed when it is read,
/// from the record the metadata store accepted, with the current statement
/// key. Ed25519 signatures are deterministic, so the same key signs the same
/// envelope every time, and a new key re-signs every statement.
/// `None` when the record cannot be read although its log entry was: its
/// version was removed meanwhile, or the read came from a replica behind
/// the one the entry came from.
async fn registry_statements(
    state: &AppState,
    logged: &LoggedRecord,
) -> pnpr_error::Result<Option<Vec<Envelope>>> {
    let Some(key) = state.inner.config.release_records.statement_signing_key.as_ref() else {
        return Ok(Some(Vec::new()));
    };
    let bytes = super::hosted_storage(state, Some(&logged.org))
        .read_release_record(&logged.name, &logged.version, 0)
        .await?;
    let Some(bytes) = bytes else { return Ok(None) };
    let invalid = |err: &dyn std::fmt::Display| pnpr_error::RegistryError::Internal {
        reason: format!("the accepted release record cannot be stated: {err}"),
    };
    let record = StoredRecord::parse(bytes).map_err(|err| invalid(&err))?;
    if record.digest().to_string() != logged.digest {
        return Err(invalid(&"its bytes are not the ones its log entry names"));
    }
    let predicate = AcceptancePredicate { interpretation: record.key().profile.clone() };
    let predicate = serde_json::to_value(predicate)?;
    let statement = Statement::about(&record, PredicateType::Acceptance, predicate)
        .map_err(|err| invalid(&err))?;
    Ok(Some(vec![statement.sign(&[key])]))
}

/// A logged release record of a hosted version.
struct LoggedRecord {
    org: String,
    name: CanonicalPackageName,
    version: String,
    /// The index of its log entry.
    index: u64,
    digest: String,
}

/// The logged record `address` names, read as the record is, or the
/// response that ends the request.
async fn logged_record(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    address: &str,
) -> Result<LoggedRecord, Response> {
    let (version, Some(0)) = super::split_packuments::record_address(address) else {
        return Err(not_found());
    };
    let name = CanonicalPackageName::parse(raw_name, Ecosystem::Npm).map_err(|_| not_found())?;
    let target = addressed_registry(state, registry, Ecosystem::Npm).ok_or_else(not_found)?;
    let RegistrySource::Hosted(source) = resolve_registry_source(state, &target, name.as_str())
    else {
        return Err(not_found());
    };
    let org = hosted_read_namespace(state, identity, &source, name.as_str())
        .map_err(IntoResponse::into_response)?;
    if super::ensure_osv_allowed(state, &name, version).is_err() {
        return Err(not_found());
    }
    if state.inner.config.release_records.log_signing_key.is_none() {
        return Err(not_found());
    }
    let database = state.inner.storage.sql_metadata();
    let entry = database.release_record_entry(&name, &org, version).await;
    let (index, digest) = entry.map_err(IntoResponse::into_response)?.ok_or_else(not_found)?;
    Ok(LoggedRecord { org, name, version: version.to_string(), index, digest })
}

/// The inclusion proof of entry `index`, waiting a bounded time for an
/// integration to cover it.
async fn wait_for_inclusion(
    state: &AppState,
    log: &LogId,
    index: u64,
) -> pnpr_error::Result<Option<InclusionProof>> {
    let deadline = tokio::time::Instant::now() + PROOF_WAIT;
    let mut asked = false;
    loop {
        if let Some(inclusion) = state.inner.storage.log_inclusion(log, index).await? {
            return Ok(Some(inclusion));
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(None);
        }
        if !asked {
            super::release_record_logs::integrate_in_background(state, log.clone());
            asked = true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The record's proof is not available yet: the client retries, and never
/// reads the record as missing or unverified.
fn proof_pending() -> Response {
    retry_later("the record's inclusion proof is pending; retry")
}

/// A `503` the client retries after a second.
fn retry_later(reason: &'static str) -> Response {
    let mut response =
        (StatusCode::SERVICE_UNAVAILABLE, axum::Json(json!({ "error": reason }))).into_response();
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    response
}

/// The Sigstore bundle published with `version`, as it was published.
async fn sigstore_bundle(
    state: &AppState,
    org: &str,
    name: &CanonicalPackageName,
    version: &str,
) -> pnpr_error::Result<Option<serde_json::Value>> {
    let filename = name.sigstore_name_for_version(version);
    let Some((body, _)) =
        super::hosted_storage(state, Some(org)).open_hosted_blob(name, &filename).await?
    else {
        return Ok(None);
    };
    let bytes = axum::body::to_bytes(body, 16 * 1024 * 1024).await
        .map_err(|err| pnpr_error::RegistryError::Io(std::io::Error::other(err)))?;
    Ok(Some(serde_json::from_slice(&bytes)?))
}

fn evidence_response(
    digest: &str,
    inclusion: &InclusionProof,
    statements: &[Envelope],
    sigstore: Option<serde_json::Value>,
) -> Response {
    let proof: Vec<String> = inclusion.proof
        .iter()
        .map(|hash| STANDARD.encode(hash.0))
        .collect();
    let body = json!({
        "record": digest,
        "inclusion": {
            "checkpoint": String::from_utf8_lossy(&inclusion.checkpoint.note),
            "index": inclusion.index,
            "proof": proof,
        },
        "statements": statements,
        "sigstore": sigstore.into_iter().collect::<Vec<_>>(),
    });
    let mut response = (StatusCode::OK, axum::Json(body)).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}
