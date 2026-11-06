use pnpm_cmd_shim::{LinkBinsError, prune_stale_bins};
use pnpm_fs::lexical_normalize;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::OnceLock,
};

/// The commands the link passes of one install shimmed into each modules
/// directory's `.bin`. Both linkers record their claims here, and a shim no
/// pass claimed is stale.
///
/// A directory is keyed by its lexically normalized path. Keys that resolve
/// to one directory on disk, such as a project reached through a symlink
/// and through its real path, are the same directory to
/// [`Self::claim_recorded`] and [`Self::prune_stale_bins`].
#[derive(Debug, Default, Clone)]
pub struct ClaimedCommands {
    by_dir: HashMap<PathBuf, HashSet<String>>,
    /// The keys by the directory each resolves to. Built on first use, so
    /// each key is resolved once, and dropped when a key is added.
    keys_by_real_dir: OnceLock<HashMap<PathBuf, Vec<PathBuf>>>,
}

impl ClaimedCommands {
    pub fn claim(&mut self, modules_dir: &Path, commands: impl IntoIterator<Item = String>) {
        self.commands_mut(modules_dir).extend(commands);
    }

    /// The commands claimed for `modules_dir`, recording the directory with
    /// no commands when nothing claimed it yet.
    pub fn commands_mut(&mut self, modules_dir: &Path) -> &mut HashSet<String> {
        let key = lexical_normalize(modules_dir);
        if !self.by_dir.contains_key(&key) {
            self.keys_by_real_dir = OnceLock::new();
        }
        self.by_dir.entry(key).or_default()
    }

    /// The commands claimed under `modules_dir` itself.
    #[must_use]
    pub fn get(&self, modules_dir: &Path) -> Option<&HashSet<String>> {
        self.by_dir.get(&lexical_normalize(modules_dir))
    }

    pub fn merge(&mut self, other: Self) {
        for (modules_dir, commands) in other.by_dir {
            self.claim(&modules_dir, commands);
        }
    }

    /// Adds `commands` to a directory a link pass already claimed: under
    /// its own key when recorded, else under the smallest key resolving to
    /// the same directory. Returns `false`, recording nothing, when no pass
    /// claimed it.
    pub fn claim_recorded(&mut self, modules_dir: &Path, commands: HashSet<String>) -> bool {
        let key = lexical_normalize(modules_dir);
        let key = if self.by_dir.contains_key(&key) {
            key
        } else {
            let Some(alias) = self
                .keys_resolving_to(&real_dir(modules_dir))
                .first()
            else {
                return false;
            };
            alias.clone()
        };
        self.commands_mut(&key).extend(commands);
        true
    }

    /// Removes from each directory's `.bin` the shims claimed under no key
    /// resolving to that directory. A directory named more than once is
    /// pruned once.
    pub fn prune_stale_bins<'a>(
        &self,
        modules_dirs: impl IntoIterator<Item = &'a Path>,
    ) -> Result<(), LinkBinsError> {
        let real_dirs: HashSet<PathBuf> = modules_dirs
            .into_iter()
            .map(real_dir)
            .collect();
        for real in real_dirs {
            let claimed: HashSet<String> = self
                .keys_resolving_to(&real)
                .iter()
                .filter_map(|key| self.by_dir.get(key))
                .flatten()
                .cloned()
                .collect();
            prune_stale_bins(&real.join(".bin"), &claimed)?;
        }
        Ok(())
    }

    /// Every recorded directory, in no particular order.
    pub fn dirs(&self) -> impl Iterator<Item = &Path> {
        self.by_dir.keys().map(PathBuf::as_path)
    }

    /// The recorded keys resolving to `real`, sorted.
    fn keys_resolving_to(&self, real: &Path) -> &[PathBuf] {
        self.keys_by_real_dir
            .get_or_init(|| {
                let mut keys: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
                for key in self.by_dir.keys() {
                    keys.entry(real_dir(key))
                        .or_default()
                        .push(key.clone());
                }
                for aliases in keys.values_mut() {
                    aliases.sort();
                }
                keys
            })
            .get(real)
            .map_or(&[], Vec::as_slice)
    }
}

/// `dir` with every symlink resolved, or normalized as written when it does
/// not exist.
fn real_dir(dir: &Path) -> PathBuf {
    std::fs::canonicalize(dir).unwrap_or_else(|_| lexical_normalize(dir))
}

#[cfg(test)]
mod tests;
