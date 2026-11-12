//! The lockfile entries a downloaded tarball satisfies, so `store add` can key
//! the tarball by ids its manifest does not reveal: a tarball URL, or a
//! git host's archive URL.

use miette::{IntoDiagnostic, WrapErr};
use pnpm_config::Config;
use pnpm_lockfile::{Lockfile, LockfileResolution};
use pnpm_store_dir::store_index_key;
use rustc_hash::FxHashMap;
use ssri::{Hash, Integrity};
use std::path::Path;

/// Every lockfile entry that pins an archive by integrity, indexed by each
/// hash of the strongest algorithm its integrity lists. A tarball matching
/// only a weaker hash of an entry does not satisfy it, since installing the
/// entry verifies the strongest one.
#[derive(Debug, Default)]
pub(super) struct LockfileEntries {
    store_index_keys_by_hash: FxHashMap<Hash, Vec<String>>,
}

impl LockfileEntries {
    /// The entries of the lockfile an install in `dir` reads. A project
    /// without a lockfile has none.
    pub(super) fn load(config: &Config, dir: &Path) -> miette::Result<Self> {
        let lockfile_dir = config.lockfile_dir_for(dir);
        let lockfile = Lockfile::load_wanted(lockfile_dir, &config.wanted_lockfile_selection())
            .into_diagnostic()
            .wrap_err_with(|| format!("read the lockfile in {}", lockfile_dir.display()))?;
        Ok(lockfile
            .as_ref()
            .map(Self::from_lockfile)
            .unwrap_or_default())
    }

    pub(super) fn from_lockfile(lockfile: &Lockfile) -> Self {
        let mut store_index_keys_by_hash = FxHashMap::<Hash, Vec<String>>::default();
        for (key, metadata) in lockfile.packages.iter().flatten() {
            let Some(integrity) = pinned_integrity(&metadata.resolution) else { continue };
            let entry_key = store_index_key(&integrity.to_string(), &key.pkg_id());
            // `Algorithm` orders the strongest first.
            let strongest = integrity.hashes
                .iter()
                .map(|hash| hash.algorithm)
                .min();
            for hash in integrity.hashes
                .iter()
                .filter(|hash| Some(hash.algorithm) == strongest)
            {
                store_index_keys_by_hash
                    .entry(hash.clone())
                    .or_default()
                    .push(entry_key.clone());
            }
        }
        LockfileEntries { store_index_keys_by_hash }
    }

    /// The `<integrity>\t<pkg_id>` key of each entry whose integrity lists a
    /// hash from `integrities`. This is the key an install reads a registry
    /// or tarball package from. A git-hosted tarball is fetched through it
    /// before the install prepares the package.
    pub(super) fn store_index_keys<'i>(
        &self,
        integrities: impl IntoIterator<Item = &'i Integrity>,
    ) -> impl Iterator<Item = &str> {
        integrities
            .into_iter()
            .flat_map(|integrity| &integrity.hashes)
            .filter_map(|hash| self.store_index_keys_by_hash.get(hash))
            .flatten()
            .map(String::as_str)
    }
}

/// The integrity of the archive an entry installs, when it pins one that a
/// package tarball can satisfy.
fn pinned_integrity(resolution: &LockfileResolution) -> Option<&Integrity> {
    match resolution {
        LockfileResolution::Registry(registry) => Some(&registry.integrity),
        LockfileResolution::Tarball(tarball) => tarball.integrity.as_ref(),
        LockfileResolution::Binary(_)
        | LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Variations(_)
        | LockfileResolution::Custom(_) => None,
    }
}

#[cfg(test)]
mod tests;
