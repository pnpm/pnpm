use axum::{
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_storage::PackageTombstone;
use serde_json::Value;

use super::super::{
    Action, AppState, RegistrySource, WriteTarget, authorize_package, hosted_storage,
    resolve_write_target,
};

pub(crate) async fn delete_package(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
) -> Response {
    let name = match CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm) {
        Ok(parsed) => parsed,
        Err(err) => return err.into_response(),
    };
    let target = match resolve_and_authorize_unpublish(state, identity, registry, &name).await {
        Ok(target) => target,
        Err(response) => return response,
    };
    let _packument_guard = state.inner.locks.packages.lock(name.as_str()).await;
    let storage = hosted_storage(state, Some(&target.org));

    let (maintainers, unpublished_versions) =
        match collect_package_tombstone_metadata(&storage, &name).await {
            Ok(metadata) => metadata,
            Err(err) => return err.into_response(),
        };

    // The tombstone is durable before anything is removed, so a removal that
    // fails partway never leaves the name claimable by someone else.
    let tombstone = Tombstone { identity, name: &name, maintainers, unpublished_versions };
    let recorded = !tombstone.is_empty();
    if recorded && let Err(err) = tombstone.write(&storage).await {
        return err.into_response();
    }
    let removed = match storage.remove_package(&name).await {
        Ok(removed) => removed,
        Err(err) => return err.into_response(),
    };
    // Archives with no document left nothing to collect; they still leave a
    // tombstone behind.
    if removed
        && !recorded
        && let Err(err) = tombstone.write(&storage).await
    {
        return err.into_response();
    }
    if removed {
        log_package_deleted(identity, &name, &target.source);
    }
    ok_created_response()
}

async fn collect_package_tombstone_metadata(
    storage: &pnpr_storage::Storage,
    name: &CanonicalPackageName,
) -> Result<(Vec<String>, Vec<String>), RegistryError> {
    let mut maintainers = Vec::new();
    let mut unpublished_versions = Vec::new();
    if let Some(existing_tombstone) = storage.read_tombstone(name).await? {
        maintainers = existing_tombstone.maintainers;
        unpublished_versions.extend(existing_tombstone.unpublished_versions);
    }
    if let Some(doc) = storage.read_hosted_document(name).await?
        && let Ok(val) = serde_json::from_slice::<Value>(&doc)
    {
        let mut live_maintainers = Vec::new();
        collect_maintainers_from_value(&val, &mut live_maintainers);
        if !live_maintainers.is_empty() {
            maintainers = live_maintainers;
        }
        collect_versions_from_value(&val, &mut unpublished_versions);
    }
    Ok((maintainers, unpublished_versions))
}

fn collect_maintainers_from_value(val: &Value, maintainers: &mut Vec<String>) {
    let Some(entries) = val.get("maintainers").and_then(Value::as_array) else { return };
    for entry in entries {
        let Some(user) = entry.get("name").and_then(Value::as_str) else { continue };
        if !maintainers
            .iter()
            .any(|existing| existing == user)
        {
            maintainers.push(user.to_string());
        }
    }
}

fn collect_versions_from_value(val: &Value, versions: &mut Vec<String>) {
    let Some(versions_obj) = val.get("versions").and_then(Value::as_object) else { return };
    for version_key in versions_obj.keys() {
        if !versions
            .iter()
            .any(|existing| existing == version_key)
        {
            versions.push(version_key.clone());
        }
    }
}

/// The tombstone a package deletion leaves.
struct Tombstone<'a> {
    identity: &'a Identity,
    name: &'a CanonicalPackageName,
    maintainers: Vec<String>,
    unpublished_versions: Vec<String>,
}

impl Tombstone<'_> {
    fn is_empty(&self) -> bool {
        self.maintainers.is_empty() && self.unpublished_versions.is_empty()
    }

    async fn write(&self, storage: &pnpr_storage::Storage) -> Result<(), RegistryError> {
        let tombstone = PackageTombstone {
            package: self.name.as_str().to_string(),
            unpublished_by: self.identity.username().map(ToString::to_string),
            unpublished_at: chrono::Utc::now().to_rfc3339(),
            maintainers: self.maintainers.clone(),
            unpublished_versions: self.unpublished_versions.clone(),
        };
        storage.write_tombstone(&tombstone).await
    }
}

pub(crate) async fn delete_tarball(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    raw_name: &str,
    filename: &str,
) -> Response {
    let name = match CanonicalPackageName::parse(raw_name, pnpr_package_name::Ecosystem::Npm) {
        Ok(parsed) => parsed,
        Err(err) => return err.into_response(),
    };
    let (canonical, version) = match name.parse_tarball_name(filename) {
        Ok(parsed) => parsed,
        Err(err) => return err.into_response(),
    };
    let target = match resolve_and_authorize_unpublish(state, identity, registry, &name).await {
        Ok(target) => target,
        Err(response) => return response,
    };
    let _packument_guard = state.inner.locks.packages.lock(name.as_str()).await;
    let storage = hosted_storage(state, Some(&target.org));
    match package_exists(&storage, &name).await {
        Ok(true) => {}
        Ok(false) => return RegistryError::NotFound.into_response(),
        Err(err) => return err.into_response(),
    }
    // A version still listed is unpublished here, in a commit that drops its
    // files' mappings; one a packument rewrite removed first went with its
    // archives then. The tombstone names it before the version goes, as a
    // rewrite's does, since a removed version is never accepted again. The
    // check reads what the last commit left, not a replica that may lag
    // behind a publish of this version on another instance.
    match storage.read_hosted_document_for_update(&name).await {
        Ok(Some(stored)) if lists_version(&stored.bytes, &version) => {}
        Ok(_) => return ok_created_response(),
        Err(err) => return err.into_response(),
    }
    if let Err(err) = record_tarball_tombstone(&storage, identity, &name, &version).await {
        return err.into_response();
    }
    match super::version_removal::remove_listed_version(&storage, identity, &name, &version).await {
        Ok(true) => log_tarball_deleted(identity, &name, &canonical, &version, &target.source),
        Ok(false) => {}
        Err(err) => return err.into_response(),
    }
    ok_created_response()
}

async fn resolve_and_authorize_unpublish(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    name: &CanonicalPackageName,
) -> Result<WriteTarget, Response> {
    let target = match resolve_write_target(state, identity, registry, name) {
        Ok(target) => target,
        Err(err) => return Err(err.into_response()),
    };
    if let Err(err) = authorize_package(
        state,
        identity,
        &RegistrySource::Hosted(target.source.clone()),
        name.as_str(),
        Action::Unpublish,
        Some(&target.org),
    )
    .await
    {
        return Err(err.into_response());
    }
    Ok(target)
}

/// Whether the packument `bytes` lists `version`.
fn lists_version(bytes: &[u8], version: &str) -> bool {
    serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|packument| {
            packument
                .get("versions")?
                .get(version)
                .cloned()
        })
        .is_some()
}

async fn package_exists(
    storage: &pnpr_storage::Storage,
    name: &CanonicalPackageName,
) -> Result<bool, RegistryError> {
    if storage.read_hosted_document(name).await?.is_some() {
        return Ok(true);
    }
    Ok(storage.read_tombstone(name).await?.is_some())
}

async fn record_tarball_tombstone(
    storage: &pnpr_storage::Storage,
    identity: &Identity,
    name: &CanonicalPackageName,
    version: &str,
) -> Result<(), RegistryError> {
    let maintainers = read_maintainers_from_storage(storage, name).await;
    let user = identity.username().map(ToString::to_string);
    let version = version.to_string();
    let name_str = name.as_str().to_string();
    storage.update_tombstone(name, move |existing| {
        let mut tombstone = match existing {
            Some(existing) => existing,
            None => PackageTombstone {
                package: name_str.clone(),
                unpublished_by: user.clone(),
                unpublished_at: chrono::Utc::now().to_rfc3339(),
                maintainers: maintainers.clone(),
                unpublished_versions: Vec::new(),
            },
        };
        if !tombstone.unpublished_versions
            .iter()
            .any(|existing| existing == &version)
        {
            tombstone.unpublished_versions.push(version.clone());
        }
        tombstone
    })
    .await?;
    Ok(())
}

async fn read_maintainers_from_storage(
    storage: &pnpr_storage::Storage,
    name: &CanonicalPackageName,
) -> Vec<String> {
    let mut maintainers = Vec::new();
    if let Ok(Some(bytes)) = storage.read_hosted_document(name).await
        && let Ok(val) = serde_json::from_slice::<Value>(&bytes)
    {
        collect_maintainers_from_value(&val, &mut maintainers);
    }
    maintainers
}

fn log_package_deleted(identity: &Identity, name: &CanonicalPackageName, source: &str) {
    let actor = identity.username().unwrap_or("-");
    tracing::info!(
        target: "pnpr::audit",
        action = "package.unpublish",
        actor = %actor,
        package = %name.as_str(),
        registry = %source,
        "entire package unpublished",
    );
}

fn log_tarball_deleted(
    identity: &Identity,
    name: &CanonicalPackageName,
    filename: &str,
    version: &str,
    source: &str,
) {
    let actor = identity.username().unwrap_or("-");
    tracing::info!(
        target: "pnpr::audit",
        action = "package.unpublish_tarball",
        actor = %actor,
        package = %name.as_str(),
        tarball = %filename,
        version = %version,
        registry = %source,
        "package tarball unpublished",
    );
}

fn ok_created_response() -> Response {
    Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"ok":true}"#))
        .expect("static-shape response always builds")
}
