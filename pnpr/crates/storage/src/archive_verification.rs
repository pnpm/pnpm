use crate::{BlobId, BoundedCache, parse_blob_key};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::Ecosystem;
use std::{
    io::{Seek, SeekFrom},
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime},
};
use tokio::fs;

const MAX_ENTRIES: usize = 4096;
// Recent writes can share one filesystem timestamp tick.
const LOCAL_TIMESTAMP_STABILITY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Fingerprint {
    Local { size: u64, modified: SystemTime, identity: FileIdentity },
    Remote { size: u64, etag: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum FileIdentity {
    #[cfg(unix)]
    Unix { device: u64, inode: u64, changed: (i64, i64) },
    #[cfg(not(unix))]
    Unavailable,
}

#[cfg(unix)]
fn file_identity(metadata: &std::fs::Metadata) -> FileIdentity {
    use std::os::unix::fs::MetadataExt;
    FileIdentity::Unix {
        device: metadata.dev(),
        inode: metadata.ino(),
        changed: (metadata.ctime(), metadata.ctime_nsec()),
    }
}

#[cfg(not(unix))]
fn file_identity(_metadata: &std::fs::Metadata) -> FileIdentity {
    FileIdentity::Unavailable
}

impl FileIdentity {
    #[cfg(unix)]
    fn has_stable_timestamp(&self, now: SystemTime) -> bool {
        let Self::Unix { changed, .. } = self;
        let Ok(seconds) = u64::try_from(changed.0) else { return false };
        let Ok(nanoseconds) = u32::try_from(changed.1) else { return false };
        let Some(changed) = std::time::UNIX_EPOCH.checked_add(Duration::new(seconds, nanoseconds))
        else {
            return false;
        };
        timestamp_is_stable(changed, now)
    }

    #[cfg(not(unix))]
    fn has_stable_timestamp(&self, _now: SystemTime) -> bool {
        let Self::Unavailable = self;
        false
    }
}

fn timestamp_is_stable(timestamp: SystemTime, now: SystemTime) -> bool {
    now.duration_since(timestamp)
        .is_ok_and(|age| age >= LOCAL_TIMESTAMP_STABILITY)
}

impl Fingerprint {
    fn has_stable_timestamps(&self, now: SystemTime) -> bool {
        match self {
            Self::Local { modified, identity, .. } => {
                timestamp_is_stable(*modified, now) && identity.has_stable_timestamp(now)
            }
            Self::Remote { .. } => true,
        }
    }

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

pub(crate) fn cached_fingerprint(key: &str) -> Option<Fingerprint> {
    cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&key.to_string())
        .cloned()
}

pub(crate) fn is_verified(key: &str, fingerprint: &Fingerprint) -> bool {
    let mut cache = cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    cached_fingerprint_matches(&mut cache, key, fingerprint)
}

fn cached_fingerprint_matches(
    cache: &mut BoundedCache<String, Fingerprint>,
    key: &str,
    fingerprint: &Fingerprint,
) -> bool {
    let key = key.to_string();
    if !cache
        .get(&key)
        .is_some_and(|cached| cached == fingerprint)
    {
        return false;
    }
    cache.insert(key, fingerprint.clone(), 1, MAX_ENTRIES);
    true
}

pub(crate) fn remember(key: String, fingerprint: Fingerprint) {
    let mut cache = cache().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    cache.insert(key, fingerprint, 1, MAX_ENTRIES);
}

pub(crate) async fn verify_file(file: fs::File, key: &str) -> Result<fs::File> {
    verify_file_with_guard(file, key, ()).await.map(|(file, ())| file)
}

pub(crate) async fn verify_file_with_guard<Guard: Send + 'static>(
    file: fs::File,
    key: &str,
    guard: Guard,
) -> Result<(fs::File, Guard)> {
    let Some((expected, _)) = parse_blob_key(key) else { return Ok((file, guard)) };
    static VERIFIERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
    let permit = VERIFIERS.acquire().await.expect("verification semaphore remains open");
    let key = key.to_string();
    let mut reader = file.into_std().await;
    let (reader, guard) = tokio::task::spawn_blocking(move || {
        let _permit = permit;
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
        Ok((reader, guard))
    })
    .await??;
    Ok((fs::File::from_std(reader), guard))
}

pub(crate) async fn verified_local_file(
    file: fs::File,
    key: &str,
    cache_key: String,
) -> Result<fs::File> {
    let fingerprint = Fingerprint::local(&file.metadata().await?)?;
    let has_stable_timestamps = fingerprint.has_stable_timestamps(SystemTime::now());
    if has_stable_timestamps && is_verified(&cache_key, &fingerprint) {
        return Ok(file);
    }
    let file = verify_file(file, key).await?;
    if Fingerprint::local(&file.metadata().await?)? != fingerprint {
        return Err(RegistryError::StorageUnavailable {
            reason: format!("stored archive {key:?} changed during verification"),
        });
    }
    if has_stable_timestamps {
        remember(cache_key, fingerprint);
    }
    Ok(file)
}

#[cfg(test)]
mod tests;
