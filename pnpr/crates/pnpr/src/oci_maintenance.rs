//! Offline OCI blob reclamation. All writers sharing the store must be
//! stopped, including other replicas, throughout the scan and deletion.

use crate::{Config, Ecosystem, RegistryError, Result};
use futures_util::TryStreamExt;
use pnpr_oci::{Digest, ImageDocument, Manifest, media_type};
use pnpr_package_name::CanonicalPackageName;
use pnpr_storage::Storage;
use rusqlite::{Connection, OptionalExtension, params};
use std::{collections::HashSet, time::Duration};

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
    collect(&storage, min_age, dry_run, &excluded, config.oci.max_manifest_bytes).await
}

async fn collect(
    storage: &Storage,
    min_age: Duration,
    dry_run: bool,
    excluded: &HashSet<&str>,
    manifest_limit: usize,
) -> Result<(usize, u64)> {
    let temporary = tempfile::NamedTempFile::new()?;
    let inventory = Connection::open(temporary.path())?;
    inventory.execute_batch(
        "PRAGMA journal_mode=OFF;
         CREATE TABLE repositories (name TEXT PRIMARY KEY);
         CREATE TABLE blobs (
             repository TEXT, filename TEXT, size INTEGER, old INTEGER,
             keep INTEGER DEFAULT 0, UNIQUE(repository, filename)
         );
         BEGIN;",
    )?;
    let mut files = storage.hosted_blob_files();
    while let Some(file) = files.try_next().await? {
        if file.path.split('/').next().is_some_and(|part| excluded.contains(part)) {
            continue;
        }
        let Some((repository, filename)) = file.path.rsplit_once('/') else { continue };
        if CanonicalPackageName::parse(repository, Ecosystem::Oci).is_err() {
            continue;
        }
        if filename != "package.json" && filename_digest(filename).is_none() {
            continue;
        }
        inventory.execute("INSERT OR IGNORE INTO repositories VALUES (?)", [repository])?;
        if filename == "package.json" {
            continue;
        }
        let size = i64::try_from(file.size).map_err(|_| RegistryError::BadRequest {
            reason: format!("blob {} is too large to inventory", file.path),
        })?;
        inventory.execute(
            "INSERT INTO blobs (repository, filename, size, old) VALUES (?, ?, ?, ?)",
            params![
                repository,
                filename,
                size,
                file.modified.elapsed().unwrap_or_default() >= min_age
            ],
        )?;
    }
    inventory.execute_batch("COMMIT;")?;
    inventory.execute_batch("BEGIN;")?;
    let mut previous = String::new();
    while let Some(repository) = inventory
        .query_row(
            "SELECT name FROM repositories WHERE name > ? ORDER BY name LIMIT 1",
            [&previous],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        let name = CanonicalPackageName::parse(&repository, Ecosystem::Oci)?;
        let reachable = referenced_blobs(storage, &name, manifest_limit).await?;
        for filename in reachable {
            inventory.execute(
                "UPDATE blobs SET keep = 1 WHERE repository = ? AND filename = ?",
                params![repository, filename],
            )?;
        }
        previous = repository;
    }
    inventory.execute_batch("COMMIT;")?;
    let mut removed = 0;
    let mut bytes = 0;
    let mut cursor = 0i64;
    loop {
        let candidate = inventory.query_row(
            "SELECT rowid, repository, filename, size FROM blobs WHERE rowid > ? AND old = 1 AND keep = 0 ORDER BY rowid LIMIT 1",
            [cursor], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?)),
        ).optional()?;
        let Some((row, repository, filename, size)) = candidate else { break };
        cursor = row;
        let size = u64::try_from(size).expect("inventory only records nonnegative blob sizes");
        let name = CanonicalPackageName::parse(&repository, Ecosystem::Oci)?;
        if dry_run || storage.remove_hosted_blob(&name, &filename).await? {
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
    if !dry_run {
        let mut previous = String::new();
        while let Some(repository) = inventory
            .query_row(
                "SELECT name FROM repositories WHERE name > ? ORDER BY name LIMIT 1",
                [&previous],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            let name = CanonicalPackageName::parse(&repository, Ecosystem::Oci)?;
            previous = repository;
            let Some(body) = storage.read_hosted_document(&name).await? else { continue };
            let mut document = ImageDocument::parse(&body)?;
            if let Some(digest) = document.deleting_blob.take() {
                let size = storage
                    .open_hosted_blob(&name, &digest.blob_filename())
                    .await?
                    .and_then(|(_, size)| size)
                    .unwrap_or_default();
                if storage.remove_hosted_blob(&name, &digest.blob_filename()).await? {
                    removed += 1;
                    bytes += size;
                }
                storage
                    .update_hosted_document_with_retry(
                        &name,
                        pnpr_storage::DOCUMENT_WRITE_RETRIES,
                        |_| Ok(Some(document.to_bytes())),
                    )
                    .await?;
            }
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
    manifest_limit: usize,
) -> Result<HashSet<String>> {
    let Some(bytes) = storage.read_hosted_document(name).await? else { return Ok(HashSet::new()) };
    let document = ImageDocument::parse(&bytes)?;
    referenced_document_blobs(storage, name, &document, manifest_limit).await
}

pub(crate) async fn referenced_document_blobs(
    storage: &Storage,
    name: &CanonicalPackageName,
    document: &ImageDocument,
    manifest_limit: usize,
) -> Result<HashSet<String>> {
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
        let bytes = axum::body::to_bytes(body, manifest_limit)
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
    if document
        .deleting_blob
        .as_ref()
        .is_some_and(|digest| reachable.contains(&digest.blob_filename()))
    {
        return Err(RegistryError::BadRequest {
            reason: format!("pending deletion in {} references a retained blob", name.as_str()),
        });
    }
    Ok(reachable)
}

#[cfg(test)]
mod tests;
