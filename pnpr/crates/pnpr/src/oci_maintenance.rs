//! Offline OCI blob reclamation. All writers sharing the store must be
//! stopped, including other replicas, throughout the scan and deletion.

use crate::{Config, Ecosystem, RegistryError, Result};
use futures_util::TryStreamExt;
use pnpr_oci::{Descriptor, Digest, ImageDocument, Manifest, media_type};
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
    let mut inventory = Inventory::open(temporary.path())?;
    inventory.execute_batch(
        "PRAGMA journal_mode=OFF;
         CREATE TABLE repositories (name TEXT PRIMARY KEY);
         CREATE TABLE blobs (
             repository TEXT, filename TEXT, size INTEGER, old INTEGER,
             keep INTEGER DEFAULT 0, UNIQUE(repository, filename)
         );
         BEGIN;",
    )?;
    inventory_hosted_blobs(storage, &mut inventory, min_age, excluded).await?;
    inventory.execute_batch("COMMIT;")?;
    inventory.execute_batch("BEGIN;")?;
    mark_reachable_blobs(storage, &mut inventory, manifest_limit).await?;
    inventory.execute_batch("COMMIT;")?;
    let (mut removed, mut bytes) =
        remove_unreferenced_blobs(storage, &mut inventory, dry_run).await?;
    if !dry_run {
        let (deleted, deleted_bytes) = finish_pending_deletions(storage, &mut inventory).await?;
        removed += deleted;
        bytes += deleted_bytes;
    }
    Ok((removed, bytes))
}

/// Record every hosted blob, with whether it is old enough to collect.
async fn inventory_hosted_blobs(
    storage: &Storage,
    inventory: &mut Inventory,
    min_age: Duration,
    excluded: &HashSet<&str>,
) -> Result<()> {
    let mut files = storage.hosted_blob_files();
    while let Some(file) = files.try_next().await? {
        let Some((repository, filename)) = inventoried_blob_path(&file.path, excluded) else {
            continue;
        };
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
    Ok(())
}

/// The repository and filename of a stored path the collector inventories, or
/// `None` for an excluded registry, a path that is not a repository, or a file
/// that is neither a document nor a digest-named blob.
fn inventoried_blob_path<'a>(
    path: &'a str,
    excluded: &HashSet<&str>,
) -> Option<(&'a str, &'a str)> {
    if path.split('/').next().is_some_and(|part| excluded.contains(part)) {
        return None;
    }
    let (repository, filename) = path.rsplit_once('/')?;
    if CanonicalPackageName::parse(repository, Ecosystem::Oci).is_err() {
        return None;
    }
    if filename != "package.json" && filename_digest(filename).is_none() {
        return None;
    }
    Some((repository, filename))
}

/// Flag every blob some manifest still reaches.
async fn mark_reachable_blobs(
    storage: &Storage,
    inventory: &mut Inventory,
    manifest_limit: usize,
) -> Result<()> {
    let mut previous = String::new();
    while let Some(repository) = next_repository(inventory, &previous)? {
        let name = CanonicalPackageName::parse(&repository, Ecosystem::Oci)?;
        for filename in referenced_blobs(storage, &name, manifest_limit).await? {
            inventory.execute(
                "UPDATE blobs SET keep = 1 WHERE repository = ? AND filename = ?",
                params![repository, filename],
            )?;
        }
        previous = repository;
    }
    Ok(())
}

/// Remove every old blob nothing reaches, reporting how many and how large.
async fn remove_unreferenced_blobs(
    storage: &Storage,
    inventory: &mut Inventory,
    dry_run: bool,
) -> Result<(usize, u64)> {
    let mut removed = 0;
    let mut bytes = 0;
    let mut cursor = 0i64;
    loop {
        let candidate = inventory.query_row(
            "SELECT rowid, repository, filename, size FROM blobs WHERE rowid > ? AND old = 1 AND keep = 0 ORDER BY rowid LIMIT 1",
            [cursor], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?)),
        )?;
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
    Ok((removed, bytes))
}

/// Finish the deletion a manifest delete left half-done: its blob is still on
/// the store, and the document still names it.
async fn finish_pending_deletions(
    storage: &Storage,
    inventory: &mut Inventory,
) -> Result<(usize, u64)> {
    let mut removed = 0;
    let mut bytes = 0;
    let mut previous = String::new();
    while let Some(repository) = next_repository(inventory, &previous)? {
        let name = CanonicalPackageName::parse(&repository, Ecosystem::Oci)?;
        previous = repository;
        let Some(body) = storage.read_hosted_document(&name).await? else { continue };
        let mut document = ImageDocument::parse(&body)?;
        let Some(digest) = document.deleting_blob.take() else { continue };
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
            .update_hosted_document_with_retry(&name, pnpr_storage::DOCUMENT_WRITE_RETRIES, |_| {
                Ok(Some(document.to_bytes()))
            })
            .await?;
    }
    Ok((removed, bytes))
}

/// The next repository of the inventory, in name order.
fn next_repository(inventory: &mut Inventory, previous: &str) -> Result<Option<String>> {
    inventory.query_row(
        "SELECT name FROM repositories WHERE name > ? ORDER BY name LIMIT 1",
        [previous],
        |row| row.get::<_, String>(0),
    )
}

/// The temporary sqlite database one collection pass records its inventory in.
///
/// Every method takes `&mut self`: a query mutates the connection's prepared
/// statement cache, and an exclusive borrow is also what keeps the futures
/// holding it across an await `Send`.
struct Inventory {
    connection: Connection,
}

#[expect(
    clippy::needless_pass_by_ref_mut,
    reason = "the exclusive borrow is what keeps a future holding the inventory across an await Send: `rusqlite::Connection` is Send but not Sync"
)]
impl Inventory {
    fn open(path: &std::path::Path) -> Result<Self> {
        Ok(Self { connection: Connection::open(path)? })
    }

    fn execute_batch(&mut self, sql: &str) -> Result<()> {
        self.connection.execute_batch(sql)?;
        Ok(())
    }

    fn execute(&mut self, sql: &str, params: impl rusqlite::Params) -> Result<usize> {
        Ok(self.connection.execute(sql, params)?)
    }

    fn query_row<Row>(
        &mut self,
        sql: &str,
        params: impl rusqlite::Params,
        row: impl FnOnce(&rusqlite::Row<'_>) -> rusqlite::Result<Row>,
    ) -> Result<Option<Row>> {
        Ok(self.connection.query_row(sql, params, row).optional()?)
    }
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
        let manifest =
            read_manifest_blob(storage, name, &digest, content_type.as_deref(), manifest_limit)
                .await?;
        for reference in manifest.references() {
            reachable.insert(reference.digest.blob_filename());
            if media_type::is_index(manifest.media_type()) {
                let content_type = resolve_reference_media_type(reference, document);
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

fn resolve_reference_media_type(
    reference: &Descriptor,
    document: &ImageDocument,
) -> Option<String> {
    reference
        .media_type
        .clone()
        .or_else(|| document.manifest(&reference.digest).map(|entry| entry.media_type.clone()))
}

/// Read one retained manifest back and parse it, checking it is the blob its
/// digest names.
async fn read_manifest_blob(
    storage: &Storage,
    name: &CanonicalPackageName,
    digest: &Digest,
    content_type: Option<&str>,
    manifest_limit: usize,
) -> Result<Manifest> {
    let filename = digest.blob_filename();
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
    if Digest::of(&bytes) != *digest {
        return Err(invalid("manifest digest mismatch".into()));
    }
    Manifest::parse(&bytes, content_type).map_err(|error| invalid(error.to_string()))
}

#[cfg(test)]
mod tests;
