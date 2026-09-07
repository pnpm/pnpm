use bytes::Bytes;
use object_store::{ObjectStoreExt as _, PutPayload};
use pnpm_shared_artifact_protocol::OwnerScope;
use pnpr_error::{RegistryError, Result};
use sha2::{Digest as _, Sha512};
use std::time::Instant;

use super::{
    ACTIVE_PUBLICATION_EXPIRY, PUBLICATION_RENEWAL_INTERVAL, SharedArtifactStore,
    artifact_operation_id, bad_request, digest_segment, owner_key,
};

pub const MAX_COMPILER_CACHE_ENTRY_SIZE: usize = 256 * 1024 * 1024;
const DIGEST_SIZE: usize = 64;

/// A relative sccache object key, including its optional namespace and shards.
#[derive(Debug)]
pub struct CompilerCacheKey(String);

impl TryFrom<String> for CompilerCacheKey {
    type Error = RegistryError;

    fn try_from(key: String) -> Result<Self> {
        if key.is_empty()
            || key.len() > 1024
            || key.split('/').any(|segment| {
                segment.is_empty()
                    || matches!(segment, "." | "..")
                    || !segment.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                    })
            })
        {
            return Err(bad_request("invalid compiler cache key".to_string()));
        }
        Ok(Self(key))
    }
}

impl SharedArtifactStore {
    /// Stores an immutable, opaque sccache entry in the named cache.
    /// The first successful publication wins, including when later bytes differ.
    /// The caller must authorize publication to this cache.
    pub async fn publish_compiler_cache(
        &self,
        cache: &str,
        key: &CompilerCacheKey,
        bytes: Bytes,
    ) -> Result<bool> {
        if bytes.len() > MAX_COMPILER_CACHE_ENTRY_SIZE {
            return Err(bad_request("compiler cache entry exceeds the size limit".to_string()));
        }
        let owner = owner_key(cache, &OwnerScope::organization(cache))?;
        let path = compiler_cache_path(&owner, key);
        if self.compiler_cache_size(cache, key).await?.is_some() {
            return Ok(false);
        }
        let publication = artifact_operation_id()?;
        self.begin_publication(&publication).await?;
        let mut reclamation_needed = false;
        let result = self
            .while_renewing(&publication, PUBLICATION_RENEWAL_INTERVAL, async {
                let started = Instant::now();
                let size = (bytes.len() + DIGEST_SIZE) as u64;
                // A failed object-store write can have committed remotely. Keep
                // its reservation until reclamation counts the actual objects.
                if let Err(error) = self.reserve_quota(&owner, size).await {
                    reclamation_needed = matches!(&error, RegistryError::ObjectStore(_));
                    return Err(error);
                }
                reclamation_needed = true;
                let digest = Bytes::copy_from_slice(&compiler_cache_digest(&owner, key, &bytes));
                let stored: PutPayload = [digest, bytes].into_iter().collect();
                let created = self.create_object(&path, stored).await?;
                self.release_uncommitted(&owner, size, if created { size } else { 0 }).await?;
                reclamation_needed = started.elapsed() >= ACTIVE_PUBLICATION_EXPIRY;
                if reclamation_needed {
                    self.begin_publication(&publication).await?;
                }
                Ok(created)
            })
            .await;
        self.complete_publication(&publication, reclamation_needed, result).await
    }

    /// Returns the payload length using object metadata, without verifying content.
    /// The caller must authorize access to this cache.
    pub async fn compiler_cache_size(
        &self,
        cache: &str,
        key: &CompilerCacheKey,
    ) -> Result<Option<u64>> {
        let owner = owner_key(cache, &OwnerScope::organization(cache))?;
        let path = self.object_path(&compiler_cache_path(&owner, key));
        match self.store.head(&path).await {
            Ok(metadata) => {
                let size = metadata
                    .size
                    .checked_sub(DIGEST_SIZE as u64)
                    .filter(|size| *size <= MAX_COMPILER_CACHE_ENTRY_SIZE as u64)
                    .ok_or_else(|| RegistryError::Internal {
                        reason: format!("stored compiler cache entry {path} has an invalid size"),
                    })?;
                Ok(Some(size))
            }
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Returns a cache entry only after verifying its stored digest against its
    /// owner, key, and contents. The caller must authorize access to this cache.
    pub async fn read_compiler_cache(
        &self,
        cache: &str,
        key: &CompilerCacheKey,
    ) -> Result<Option<Bytes>> {
        let owner = owner_key(cache, &OwnerScope::organization(cache))?;
        let path = compiler_cache_path(&owner, key);
        let Some(stored) = self
            .read_object_bounded(&path, (MAX_COMPILER_CACHE_ENTRY_SIZE + DIGEST_SIZE) as u64)
            .await?
        else {
            return Ok(None);
        };
        let stored = Bytes::from(stored);
        if stored.len() < DIGEST_SIZE
            || stored[..DIGEST_SIZE] != compiler_cache_digest(&owner, key, &stored[DIGEST_SIZE..])
        {
            return Err(RegistryError::Internal {
                reason: format!("stored compiler cache entry {path} failed integrity verification"),
            });
        }
        Ok(Some(stored.slice(DIGEST_SIZE..)))
    }
}

fn compiler_cache_path(owner: &str, key: &CompilerCacheKey) -> String {
    format!("{owner}/compiler-cache/v1/{}", digest_segment(key.0.as_bytes()))
}

fn compiler_cache_digest(owner: &str, key: &CompilerCacheKey, bytes: &[u8]) -> [u8; DIGEST_SIZE] {
    Sha512::new()
        .chain_update(b"pnpr-compiler-cache:v1\0")
        .chain_update(owner)
        .chain_update(b"\0")
        .chain_update(&key.0)
        .chain_update(b"\0")
        .chain_update(bytes)
        .finalize()
        .into()
}

#[cfg(test)]
mod tests;
