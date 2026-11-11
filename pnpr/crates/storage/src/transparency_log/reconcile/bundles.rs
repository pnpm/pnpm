//! Entry bundle bounds, decoding, and log-entry parsing for log recovery.

use crate::transparency_log::{ImportedLogEntry, LogId, StoredCheckpoint};
use pnpm_release_record::{LogEntry, RecordIdentity};
use pnpm_transparency_log::{TILE_WIDTH, TilePath, decode_bundle};
use pnpr_error::{RegistryError, Result};
use pnpr_package_name::Ecosystem;
use std::collections::HashSet;

pub(super) fn check_prior_full_bundle_keys(
    log: &LogId,
    start_bundle: u64,
    keys: Vec<String>,
) -> Result<()> {
    let present: HashSet<u64> = keys
        .into_iter()
        .filter_map(|key| format!("tile/entries/{key}").parse::<TilePath>().ok())
        .filter_map(|path| match path {
            TilePath::Entries { index, width: TILE_WIDTH } if index < start_bundle => Some(index),
            TilePath::Hashes { .. } | TilePath::Entries { .. } => None,
        })
        .collect();
    for index in 0..start_bundle {
        if !present.contains(&index) {
            let path = TilePath::Entries { index, width: TILE_WIDTH };
            return Err(RegistryError::Internal {
                reason: format!("the stored entry bundle {path} of the log {log} is missing"),
            });
        }
    }
    Ok(())
}

pub(super) fn max_partial_width_of_keys(
    index: u64,
    prefix: &str,
    keys: Vec<String>,
) -> Option<u32> {
    keys.into_iter()
        .filter_map(|name| format!("{prefix}/{name}").parse::<TilePath>().ok())
        .filter_map(|path| match path {
            TilePath::Entries { index: found, width } if found == index && width < TILE_WIDTH => {
                Some(width)
            }
            TilePath::Hashes { .. } | TilePath::Entries { .. } => None,
        })
        .max()
}

pub(super) fn check_authenticated_bound(
    log: &LogId,
    path: TilePath,
    bundle_end: u64,
    authenticated_end: u64,
) -> Result<()> {
    if bundle_end > authenticated_end {
        return Err(RegistryError::Internal {
            reason: format!(
                "the stored entry bundle {path} of the log {log} extends to {bundle_end} entries, beyond its {authenticated_end} authenticated entries",
            ),
        });
    }
    Ok(())
}

pub(super) fn check_bundle_bounds(
    log: &LogId,
    head_size: u64,
    checkpoint: Option<&StoredCheckpoint>,
    bundled_end: u64,
) -> Result<()> {
    if bundled_end < head_size {
        return Err(RegistryError::Internal {
            reason: format!(
                "the stored entry bundles of the log {log} cover {bundled_end} entries, fewer than its integrated tree of {head_size}",
            ),
        });
    }
    if let Some(stored) = checkpoint
        && stored.tree_head.size > bundled_end
    {
        return Err(RegistryError::Internal {
            reason: format!(
                "the stored checkpoint of the log {log} covers {} entries, more than its stored entry bundles ({bundled_end})",
                stored.tree_head.size,
            ),
        });
    }
    Ok(())
}

pub(super) fn decode_bundle_exact(
    log: &LogId,
    path: TilePath,
    bytes: &[u8],
) -> Result<Vec<Vec<u8>>> {
    let expected = match path {
        TilePath::Entries { width, .. } => usize::try_from(width).unwrap_or_default(),
        TilePath::Hashes { .. } => 0,
    };
    let decoded = decode_bundle(bytes)
        .map_err(|err| RegistryError::Internal {
            reason: format!("the stored entry bundle {path} of the log {log} is malformed: {err}"),
        })?;
    if decoded.len() != expected {
        return Err(RegistryError::Internal {
            reason: format!(
                "the stored entry bundle {path} of the log {log} holds {} entries, expected {expected}",
                decoded.len(),
            ),
        });
    }
    Ok(decoded
        .into_iter()
        .map(<[u8]>::to_vec)
        .collect())
}

pub(super) fn parse_imported_entries(
    log: &LogId,
    start: u64,
    raw_entries: &[Vec<u8>],
) -> Result<Vec<ImportedLogEntry>> {
    raw_entries
        .iter()
        .enumerate()
        .map(|(offset, bytes)| {
            let index = start + u64::try_from(offset).unwrap_or_default();
            parse_one_imported_entry(log, index, bytes)
        })
        .collect()
}

fn parse_one_imported_entry(log: &LogId, index: u64, bytes: &[u8]) -> Result<ImportedLogEntry> {
    let entry = LogEntry::parse(bytes)
        .map_err(|err| RegistryError::Internal {
            reason: format!(
                "entry {index} in the stored bundle of the log {log} is invalid: {err}",
            ),
        })?;
    let key = entry.key();
    if entry_ecosystem(&key.identity) != Some(log.ecosystem) {
        return Err(RegistryError::Internal {
            reason: format!(
                "entry {index} in the stored bundle of the log {log} describes {}, outside {}",
                key.subject_name(),
                log.ecosystem,
            ),
        });
    }
    let npm_version = match (&key.identity, key.revision) {
        (RecordIdentity::Package(purl), Some(0)) if log.ecosystem == Ecosystem::Npm => {
            purl.npm_package()
        }
        _ => None,
    };
    Ok(ImportedLogEntry { bytes: bytes.to_vec(), npm_version })
}

fn entry_ecosystem(identity: &RecordIdentity) -> Option<Ecosystem> {
    match identity {
        RecordIdentity::Package(purl) => match purl.purl_type() {
            "npm" => Some(Ecosystem::Npm),
            "cargo" => Some(Ecosystem::Cargo),
            "pypi" => Some(Ecosystem::Pypi),
            _ => None,
        },
        RecordIdentity::OciTag { .. } => Some(Ecosystem::Oci),
    }
}
