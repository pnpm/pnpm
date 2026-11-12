use crate::{
    Config,
    config_validation::{
        CacheDirCheck, MAX_CACHE_SECRET_MARKER_BYTES, PENDING_CACHE_CLAIM,
        ensure_only_ignorable_entries,
    },
};
use pnpm_fs::OpenDir;
use pnpr_config::{CACHE_SECRET_MARKER, cache_secret_fingerprint};
use std::{
    ffi::OsStr,
    io::{self, ErrorKind},
    path::Path,
    sync::Arc,
    time::Duration,
};

enum ClaimAttempt {
    Claimed,
    RetryAfterRollback,
}

enum ExclusiveClaim {
    Claimed,
    AlreadyExists,
}

enum MarkerRead {
    Missing,
    Symlink,
    Contents(String),
}

pub(super) async fn claim_ephemeral_cache_dir(config: &Config) -> pnpr_error::Result<()> {
    crate::config_validation::validate_cache_paths_and_backend(config)?;
    if config.has_configured_secret {
        return Ok(());
    }
    let cache_dir = &config.storage.cache_dir;
    tokio::fs::create_dir_all(cache_dir).await?;
    let dir = open_anchored_cache_dir(cache_dir).await?;
    crate::config_validation::validate_cache_paths_and_backend(config)?;
    let fingerprint = cache_secret_fingerprint(&config.cache_secret);
    for _ in 0..2 {
        match try_claim_ephemeral_marker(&dir, cache_dir, &fingerprint).await? {
            ClaimAttempt::Claimed => return Ok(()),
            ClaimAttempt::RetryAfterRollback => {}
        }
    }
    Err(persistent_cache_secret_error())
}

async fn open_anchored_cache_dir(cache_dir: &Path) -> pnpr_error::Result<Arc<OpenDir>> {
    let target = cache_dir.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let dir = OpenDir::open(&target)?;
        ensure_anchored_identity(&dir, &target)?;
        Ok(Arc::new(dir))
    })
    .await
    .map_err(|err| join_error(&err))?
}

async fn try_claim_ephemeral_marker(
    dir: &Arc<OpenDir>,
    cache_dir: &Path,
    fingerprint: &str,
) -> pnpr_error::Result<ClaimAttempt> {
    let handle = Arc::clone(dir);
    let target = cache_dir.to_path_buf();
    let expected = fingerprint.to_string();
    let outcome = tokio::task::spawn_blocking(move || {
        match handle.create_child_file_new(
            OsStr::new(CACHE_SECRET_MARKER),
            PENDING_CACHE_CLAIM.as_bytes(),
        ) {
            Ok(()) => finalize_pending_claim_sync(&handle, &target, &expected)
                .map(|()| ExclusiveClaim::Claimed),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => Ok(ExclusiveClaim::AlreadyExists),
            Err(err) => Err(err.into()),
        }
    })
    .await
    .map_err(|err| join_error(&err))??;
    match outcome {
        ExclusiveClaim::Claimed => return Ok(ClaimAttempt::Claimed),
        ExclusiveClaim::AlreadyExists => {}
    }
    match await_completed_marker(dir).await? {
        Some(stored) if stored == fingerprint => {
            verify_anchored_identity(dir, cache_dir).await?;
            Ok(ClaimAttempt::Claimed)
        }
        Some(_) => Err(persistent_cache_secret_error()),
        None => Ok(ClaimAttempt::RetryAfterRollback),
    }
}

fn finalize_pending_claim_sync(
    dir: &OpenDir,
    cache_dir: &Path,
    fingerprint: &str,
) -> pnpr_error::Result<()> {
    let marker = OsStr::new(CACHE_SECRET_MARKER);
    let outcome = ensure_only_ignorable_entries(dir, CacheDirCheck::ClaimedWithMarker)
        .and_then(|()| ensure_anchored_identity(dir, cache_dir))
        .and_then(|()| {
            dir.write_child_file(marker, fingerprint.as_bytes())
                .map_err(pnpr_error::RegistryError::from)
        })
        .and_then(|()| ensure_anchored_identity(dir, cache_dir));
    if outcome.is_err() {
        let _ = dir.remove_child_file(marker);
    }
    outcome
}

async fn verify_anchored_identity(dir: &Arc<OpenDir>, cache_dir: &Path) -> pnpr_error::Result<()> {
    let handle = Arc::clone(dir);
    let target = cache_dir.to_path_buf();
    tokio::task::spawn_blocking(move || ensure_anchored_identity(&handle, &target))
        .await
        .map_err(|err| join_error(&err))?
}

async fn await_completed_marker(dir: &Arc<OpenDir>) -> pnpr_error::Result<Option<String>> {
    for _ in 0..50 {
        match read_marker_state(dir).await? {
            MarkerRead::Missing => return Ok(None),
            MarkerRead::Symlink => return Err(persistent_cache_secret_error()),
            MarkerRead::Contents(raw) if raw == PENDING_CACHE_CLAIM => {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            MarkerRead::Contents(raw) => return Ok(Some(raw)),
        }
    }
    Ok(Some(PENDING_CACHE_CLAIM.to_string()))
}

async fn read_marker_state(dir: &Arc<OpenDir>) -> pnpr_error::Result<MarkerRead> {
    let handle = Arc::clone(dir);
    tokio::task::spawn_blocking(move || {
        let marker = OsStr::new(CACHE_SECRET_MARKER);
        if handle.has_symlink_child(marker)? {
            return Ok(MarkerRead::Symlink);
        }
        let bytes = match handle.read_child_file_bounded(marker, MAX_CACHE_SECRET_MARKER_BYTES) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Ok(MarkerRead::Missing),
            Err(err) if err.kind() == ErrorKind::InvalidData => {
                return Ok(MarkerRead::Contents("<oversized>".to_string()));
            }
            Err(err) => return Err(err.into()),
        };
        let parsed = std::str::from_utf8(&bytes)
            .map(|raw| raw.trim().to_string())
            .unwrap_or_else(|_| "<invalid>".to_string());
        Ok(MarkerRead::Contents(parsed))
    })
    .await
    .map_err(|err| join_error(&err))?
}

fn ensure_anchored_identity(dir: &OpenDir, configured_cache: &Path) -> pnpr_error::Result<()> {
    let Some(anchored) = dir.canonical_path()? else {
        return Err(retargeted_cache_error());
    };
    let current = pnpm_fs::resolve_path_with_symlinks(configured_cache)
        .map_err(|_| retargeted_cache_error())?;
    if anchored != current {
        return Err(retargeted_cache_error());
    }
    Ok(())
}

fn retargeted_cache_error() -> pnpr_error::RegistryError {
    pnpr_error::RegistryError::Io(io::Error::new(
        ErrorKind::InvalidInput,
        "cache directory path changed while claiming ephemeral cache",
    ))
}

fn join_error(err: &tokio::task::JoinError) -> pnpr_error::RegistryError {
    pnpr_error::RegistryError::Io(io::Error::other(err.to_string()))
}

fn persistent_cache_secret_error() -> pnpr_error::RegistryError {
    pnpr_error::RegistryError::InvalidConfig {
        reason: crate::config_validation::PERSISTENT_CACHE_SECRET_REQUIRED.to_string(),
    }
}
