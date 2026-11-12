use crate::Config;
use std::{path::Path, time::Duration};

const PENDING_CACHE_CLAIM: &str = "pending";

enum ClaimAttempt {
    Claimed,
    RetryAfterRollback,
}

pub(super) async fn claim_ephemeral_cache_dir(config: &Config) -> pnpr_error::Result<()> {
    crate::config_validation::validate_cache_paths(config)?;
    if config.has_configured_secret {
        return Ok(());
    }
    let cache_dir = &config.storage.cache_dir;
    tokio::fs::create_dir_all(cache_dir).await?;
    let marker = cache_dir.join(pnpr_config::CACHE_SECRET_MARKER);
    let fingerprint = pnpr_config::cache_secret_fingerprint(&config.cache_secret);
    for _ in 0..2 {
        match try_claim_ephemeral_marker(cache_dir, &marker, &fingerprint).await? {
            ClaimAttempt::Claimed => return Ok(()),
            ClaimAttempt::RetryAfterRollback => {}
        }
    }
    Err(persistent_cache_secret_error())
}

async fn try_claim_ephemeral_marker(
    cache_dir: &Path,
    marker: &Path,
    fingerprint: &str,
) -> pnpr_error::Result<ClaimAttempt> {
    match pnpr_storage::write_atomic_new(marker, PENDING_CACHE_CLAIM.as_bytes()).await {
        Ok(()) => finalize_pending_claim(cache_dir, marker, fingerprint).await,
        Err(pnpr_error::RegistryError::Io(err))
            if err.kind() == std::io::ErrorKind::AlreadyExists =>
        {
            match await_completed_marker(marker).await? {
                Some(stored) if stored == fingerprint => Ok(ClaimAttempt::Claimed),
                Some(_) => Err(persistent_cache_secret_error()),
                None => Ok(ClaimAttempt::RetryAfterRollback),
            }
        }
        Err(err) => Err(err),
    }
}

async fn finalize_pending_claim(
    cache_dir: &Path,
    marker: &Path,
    fingerprint: &str,
) -> pnpr_error::Result<ClaimAttempt> {
    if let Err(err) = ensure_only_ephemeral_entries(cache_dir).await {
        let _ = tokio::fs::remove_file(marker).await;
        return Err(err);
    }
    if let Err(err) = pnpr_storage::write_atomic(marker, fingerprint.as_bytes()).await {
        let _ = tokio::fs::remove_file(marker).await;
        return Err(err);
    }
    Ok(ClaimAttempt::Claimed)
}

async fn await_completed_marker(marker: &Path) -> pnpr_error::Result<Option<String>> {
    for _ in 0..50 {
        match tokio::fs::read_to_string(marker).await {
            Ok(raw) if raw.trim() == PENDING_CACHE_CLAIM => {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Ok(raw) => return Ok(Some(raw.trim().to_string())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        }
    }
    Ok(Some(PENDING_CACHE_CLAIM.to_string()))
}

async fn ensure_only_ephemeral_entries(cache_dir: &Path) -> pnpr_error::Result<()> {
    use crate::config_validation::{CacheDirCheck, is_ignorable_cache_entry};

    let mut entries = tokio::fs::read_dir(cache_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        if !is_ignorable_cache_entry(&entry.file_name(), CacheDirCheck::ClaimedWithMarker) {
            return Err(persistent_cache_secret_error());
        }
    }
    Ok(())
}

fn persistent_cache_secret_error() -> pnpr_error::RegistryError {
    pnpr_error::RegistryError::InvalidConfig {
        reason: crate::config_validation::PERSISTENT_CACHE_SECRET_REQUIRED.to_string(),
    }
}
