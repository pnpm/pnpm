use super::add_cargo_checksum;
use miette::Result;
use pnpm_store_dir::{
    PackageFilesIndex, SharedReadonlyStoreIndex, StoreDir, StoreIndexWriter, VerifiedFilesCache,
    check_pkg_files_integrity,
};
use std::{collections::HashMap, path::PathBuf};

const CHECKSUM_FILE: &str = ".cargo-checksum.json";

pub(super) struct ChecksumCache<'a> {
    pub store_dir: &'a StoreDir,
    pub index: Option<&'a SharedReadonlyStoreIndex>,
    pub writer: &'a StoreIndexWriter,
    pub verified_files: &'a VerifiedFilesCache,
}

impl ChecksumCache<'_> {
    /// The source CAS files must have passed the install's integrity policy
    /// before their paths are used to identify a checksum projection.
    pub(super) fn add(
        &self,
        cas_paths: &mut HashMap<String, PathBuf>,
        package_checksum: &str,
    ) -> Result<()> {
        cas_paths.remove(CHECKSUM_FILE);
        let key = checksum_cache_key(cas_paths, package_checksum);
        if let Some(path) = self.read(&key) {
            cas_paths.insert(CHECKSUM_FILE.to_string(), path);
            return Ok(());
        }
        let info = add_cargo_checksum(self.store_dir, cas_paths, Some(package_checksum))?;
        self.writer.queue(
            key,
            PackageFilesIndex {
                algo: "sha512".to_string(),
                files: HashMap::from([(CHECKSUM_FILE.to_string(), info)]),
                ..PackageFilesIndex::default()
            },
        );
        Ok(())
    }

    fn read(&self, key: &str) -> Option<PathBuf> {
        let entry = {
            let index = self.index?;
            let guard = index.lock().ok()?;
            match guard.get(key) {
                Ok(entry) => entry?,
                Err(error) => {
                    tracing::debug!(?error, key, "Cargo checksum cache lookup failed");
                    return None;
                }
            }
        };
        let mut verified = check_pkg_files_integrity(self.store_dir, entry, self.verified_files);
        verified.passed.then(|| verified.files_map.remove(CHECKSUM_FILE)).flatten()
    }
}

fn checksum_cache_key(cas_paths: &HashMap<String, PathBuf>, package_checksum: &str) -> String {
    let mut input = package_checksum.as_bytes().to_vec();
    input.push(0);
    let mut files = cas_paths.iter().collect::<Vec<_>>();
    files.sort_unstable_by_key(|(name, _)| *name);
    for (name, path) in files {
        for bytes in [name.as_bytes(), path.as_os_str().as_encoded_bytes()] {
            input.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            input.extend_from_slice(bytes);
        }
    }
    format!("cargo-checksum-v1:{}", pnpm_crypto_hash::create_hex_hash_bytes(&input))
}

#[cfg(test)]
mod tests;
