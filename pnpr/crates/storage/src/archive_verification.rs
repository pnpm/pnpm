use crate::{BlobId, BoundedCache, parse_blob_key};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::Ecosystem;
use std::{
    io::{Seek, SeekFrom},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};
use tokio::fs;

const MAX_ENTRIES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Fingerprint {
    Local { size: u64, modified: SystemTime, identity: FileIdentity },
    Remote { size: u64, etag: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct FileIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    changed: (i64, i64),
    #[cfg(not(unix))]
    created: Option<SystemTime>,
}

fn file_identity(metadata: &std::fs::Metadata) -> FileIdentity {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        FileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        }
    }
    #[cfg(not(unix))]
    {
        FileIdentity { created: metadata.created().ok() }
    }
}

impl Fingerprint {
    pub(crate) fn local(metadata: &std::fs::Metadata) -> Result<Self> {
        Ok(Self::Local {
            size: metadata.len(),
            modified: metadata.modified()?,
            identity: file_identity(metadata),
        })
    }
}

fn cache() -> &'static Mutex<BoundedCache<String, Fingerprint>> {
    static CACHE: OnceLock<Mutex<BoundedCache<String, Fingerprint>>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

pub(crate) fn is_verified(key: &str, fingerprint: &Fingerprint) -> bool {
    cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&key.to_string())
        .is_some_and(|cached| cached == fingerprint)
}

pub(crate) fn remember(key: String, fingerprint: Fingerprint) {
    let mut cache = cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    cache.insert(key, fingerprint, 1, MAX_ENTRIES);
}

pub(crate) async fn verify_file(mut file: fs::File, key: &str) -> Result<fs::File> {
    let Some((expected, _)) = parse_blob_key(key) else { return Ok(file) };
    let key = key.to_string();
    let mut reader = file.into_std().await;
    reader = tokio::task::spawn_blocking(move || {
        let ecosystem = match expected
            .as_str()
            .split_once('-')
            .map(|(algorithm, _)| algorithm)
        {
            Some("sha512") => Ecosystem::Npm,
            _ => Ecosystem::Oci,
        };
        let actual = BlobId::of_reader(ecosystem, &mut reader)?;
        if actual != expected {
            return Err(RegistryError::CorruptArchive { key });
        }
        reader.seek(SeekFrom::Start(0))?;
        Ok(reader)
    })
    .await??;
    file = fs::File::from_std(reader);
    Ok(file)
}

pub(crate) async fn verified_local_file(
    file: fs::File,
    key: &str,
    cache_key: String,
) -> Result<fs::File> {
    let fingerprint = Fingerprint::local(&file.metadata().await?)?;
    if is_verified(&cache_key, &fingerprint) {
        return Ok(file);
    }
    let file = verify_file(file, key).await?;
    if Fingerprint::local(&file.metadata().await?)? != fingerprint {
        return Err(RegistryError::StorageUnavailable {
            reason: format!("stored archive {key:?} changed during verification"),
        });
    }
    remember(cache_key, fingerprint);
    Ok(file)
}

#[cfg(test)]
mod tests;
