//! File-level diff between a resolved dependency tree and what the
//! client already has in its content-addressable store.
//!
//! Port of the pnpm-agent TypeScript `diff.ts`. Given a resolved
//! lockfile, the client's store integrities, and the server's store
//! index, it computes which individual files the client is missing —
//! deduplicated by `(digest, executable)` — plus the per-package
//! msgpack index entries the client needs to write into its own store
//! index before a headless install.

use std::collections::{HashMap, HashSet};

use pacquet_store_dir::{
    PackageFilesIndex, StoreIndex, StoreIndexError, encode_package_files_index,
};

/// One resolved package the diff considers, distilled from the
/// lockfile's `packages` map.
pub struct ResolvedPackage {
    /// SRI integrity string, e.g. `sha512-...`.
    pub integrity: String,
    /// Package id without the peer-deps suffix, e.g. `foo@1.0.0`.
    pub pkg_id: String,
}

/// A file the client's store is missing.
pub struct MissingFile {
    /// Lowercase sha512 hex digest (no `sha512-` prefix).
    pub digest: String,
    pub size: u64,
    pub executable: bool,
}

/// Pre-packed store-index entry forwarded to the client (`I` line).
pub struct PackageIndexEntry {
    pub integrity: String,
    pub pkg_id: String,
    /// msgpackr-records bytes ready for the client's `StoreIndex.setRawMany`.
    pub raw: Vec<u8>,
}

#[derive(Default)]
pub struct Stats {
    pub total_packages: u64,
    pub already_in_store: u64,
    pub packages_to_fetch: u64,
    pub files_in_new_packages: u64,
    pub files_already_in_cafs: u64,
    pub files_to_download: u64,
    pub download_bytes: u64,
}

pub struct DiffResult {
    pub missing_files: Vec<MissingFile>,
    pub package_index: Vec<PackageIndexEntry>,
    pub stats: Stats,
}

struct IntegrityEntry {
    decoded: PackageFilesIndex,
    /// Re-encoded msgpackr-records buffer for the client.
    raw: Vec<u8>,
}

fn is_executable(mode: u32) -> bool {
    mode & 0o111 != 0
}

/// Build a map from SRI integrity to its decoded files index and a
/// re-encoded msgpackr-records buffer, restricted to the integrities
/// the diff actually needs (the client's existing packages plus the
/// newly resolved ones). Re-encoding guarantees the buffer is in the
/// msgpackr-records shape the pnpm client's store index reads, no
/// matter whether pacquet wrote the row as plain msgpack.
///
/// The server's store index keys are `{integrity}\t{pkgId}`; we key by
/// the integrity half and keep the first occurrence, matching
/// `buildIntegrityIndex` in the TypeScript agent.
fn build_integrity_index(
    store: &StoreIndex,
    needed: &HashSet<String>,
) -> Result<HashMap<String, IntegrityEntry>, StoreIndexError> {
    let mut index = HashMap::new();
    for key in store.keys()? {
        let Some((integrity, _pkg_id)) = key.split_once('\t') else { continue };
        if !needed.contains(integrity) || index.contains_key(integrity) {
            continue;
        }
        let Some(decoded) = store.get(&key)? else { continue };
        let Ok(raw) = encode_package_files_index(&decoded) else { continue };
        index.insert(integrity.to_string(), IntegrityEntry { decoded, raw });
    }
    Ok(index)
}

/// Compute the file-level diff. Mirrors `computeDiff` in the
/// TypeScript agent: union the client's existing file digests, then
/// for every resolved package not already in the client's store emit
/// the files it doesn't yet have (deduped across the whole response).
pub fn compute_diff(
    store: &StoreIndex,
    packages: &[ResolvedPackage],
    store_integrities: &[String],
) -> Result<DiffResult, StoreIndexError> {
    let mut needed: HashSet<String> = store_integrities.iter().cloned().collect();
    for pkg in packages {
        needed.insert(pkg.integrity.clone());
    }
    let index = build_integrity_index(store, &needed)?;

    let client_integrities: HashSet<&str> = store_integrities.iter().map(String::as_str).collect();

    // Digests (and their exec flag) the client already has on disk.
    let mut client_digests: HashSet<(String, bool)> = HashSet::new();
    for integrity in store_integrities {
        let Some(entry) = index.get(integrity) else { continue };
        for file in entry.decoded.files.values() {
            client_digests.insert((file.digest.clone(), is_executable(file.mode)));
        }
    }

    let mut stats = Stats::default();
    let mut missing_files = Vec::new();
    let mut package_index = Vec::new();

    for pkg in packages {
        stats.total_packages += 1;

        if client_integrities.contains(pkg.integrity.as_str()) {
            stats.already_in_store += 1;
            continue;
        }

        let Some(entry) = index.get(&pkg.integrity) else { continue };
        stats.packages_to_fetch += 1;

        for file in entry.decoded.files.values() {
            stats.files_in_new_packages += 1;
            let executable = is_executable(file.mode);
            let key = (file.digest.clone(), executable);
            if client_digests.insert(key) {
                stats.files_to_download += 1;
                stats.download_bytes += file.size;
                missing_files.push(MissingFile {
                    digest: file.digest.clone(),
                    size: file.size,
                    executable,
                });
            } else {
                stats.files_already_in_cafs += 1;
            }
        }

        package_index.push(PackageIndexEntry {
            integrity: pkg.integrity.clone(),
            pkg_id: pkg.pkg_id.clone(),
            raw: entry.raw.clone(),
        });
    }

    Ok(DiffResult { missing_files, package_index, stats })
}
