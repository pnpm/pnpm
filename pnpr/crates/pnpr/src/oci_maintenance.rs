//! Offline OCI blob reclamation. All writers sharing the store must be
//! stopped, including other replicas, throughout the scan and deletion.

use crate::{Config, Ecosystem, RegistryError, Result};
use pnpr_oci::{Digest, ImageDocument, Manifest, media_type};
use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::{HostedBlobFile, Storage};
use std::{
    collections::{BTreeMap, HashSet},
    time::Duration,
};

/// Reclaim unreferenced OCI blobs in one hosted registry.
///
/// Stop every writer sharing this store before calling this function. Recover
/// publish journals first, including those on other replicas' scratch volumes.
/// `dry_run` reports candidates without removing them. Blobs newer than
/// `min_age` are retained so an interrupted push can resume after maintenance.
pub async fn collect_oci_blobs(
    config: &Config,
    registry: &str,
    min_age: Duration,
    dry_run: bool,
) -> Result<(usize, u64)> {
    if config.registries.ecosystem(registry) != Some(Ecosystem::Oci) {
        return Err(RegistryError::BadRequest {
            reason: format!("{registry:?} is not a concrete OCI registry"),
        });
    }
    let hosted = config.hosted.get(registry).ok_or_else(|| RegistryError::BadRequest {
        reason: format!("{registry:?} is not a hosted registry"),
    })?;
    let storage =
        Storage::new(&config.hosted_store, config.storage.clone(), config.cache_storage.clone())?
            .for_hosted(&hosted.org);
    let excluded: HashSet<&str> = if hosted.org.is_empty() {
        config
            .hosted
            .values()
            .map(|hosted| hosted.org.as_str())
            .filter(|org| !org.is_empty())
            .collect()
    } else {
        HashSet::new()
    };
    collect(&storage, min_age, dry_run, &excluded).await
}

async fn collect(
    storage: &Storage,
    min_age: Duration,
    dry_run: bool,
    excluded: &HashSet<&str>,
) -> Result<(usize, u64)> {
    let mut repositories: BTreeMap<String, Vec<HostedBlobFile>> = BTreeMap::new();
    for file in storage.hosted_blob_files().await? {
        if file.path.split('/').next().is_some_and(|part| excluded.contains(part)) {
            continue;
        }
        let Some((repository, filename)) = file.path.rsplit_once('/') else { continue };
        if filename_digest(filename).is_none() {
            continue;
        }
        repositories.entry(repository.to_string()).or_default().push(file);
    }
    let mut candidates = Vec::new();
    for (repository, files) in repositories {
        let Ok(name) = CanonicalPackageName::parse(&repository, Ecosystem::Oci) else { continue };
        let reachable = referenced_blobs(storage, &name).await?;
        for file in files {
            let filename = file.path.rsplit('/').next().expect("inventory entry has a filename");
            if !reachable.contains(filename)
                && file.modified.elapsed().unwrap_or_default() >= min_age
            {
                candidates.push((name.clone(), filename.to_string(), file.size));
            }
        }
    }
    let mut removed = 0;
    let mut bytes = 0;
    for (name, filename, size) in candidates {
        if dry_run || storage.remove_blob(&name, &filename).await? {
            tracing::info!(
                repository = name.as_str(),
                blob = filename,
                size,
                dry_run,
                "unreferenced OCI blob",
            );
            removed += 1;
            bytes += size;
        }
    }
    Ok((removed, bytes))
}

fn filename_digest(filename: &str) -> Option<Digest> {
    Digest::parse(&format!("sha256:{}", filename.strip_prefix("sha256-")?)).ok()
}

async fn referenced_blobs(
    storage: &Storage,
    name: &CanonicalPackageName,
) -> Result<HashSet<String>> {
    let Some(bytes) = storage.read_hosted_document(name).await? else { return Ok(HashSet::new()) };
    let document = ImageDocument::parse(&bytes)?;
    let mut pending: Vec<(Digest, Option<String>)> = document
        .manifests()
        .iter()
        .map(|entry| (entry.digest.clone(), Some(entry.media_type.clone())))
        .collect();
    let mut visited = HashSet::new();
    let mut reachable = HashSet::new();
    while let Some((digest, content_type)) = pending.pop() {
        if !visited.insert(digest.clone()) {
            continue;
        }
        let filename = digest.blob_filename();
        reachable.insert(filename.clone());
        let invalid = |reason: String| RegistryError::BadRequest {
            reason: format!("cannot collect {}/{filename}: {reason}", name.as_str()),
        };
        let (body, _) = storage
            .open_hosted_blob(name, &filename)
            .await?
            .ok_or_else(|| invalid("retained manifest is missing".into()))?;
        let bytes = axum::body::to_bytes(body, 4 * 1024 * 1024)
            .await
            .map_err(|error| invalid(error.to_string()))?;
        if Digest::of(&bytes) != digest {
            return Err(invalid("manifest digest mismatch".into()));
        }
        let manifest = Manifest::parse(&bytes, content_type.as_deref())
            .map_err(|error| invalid(error.to_string()))?;
        for reference in manifest.references() {
            reachable.insert(reference.digest.blob_filename());
            if media_type::is_index(manifest.media_type()) {
                let content_type = reference.media_type.clone().or_else(|| {
                    document.manifest(&reference.digest).map(|entry| entry.media_type.clone())
                });
                pending.push((reference.digest.clone(), content_type));
            }
        }
    }
    Ok(reachable)
}

#[cfg(test)]
mod tests;
