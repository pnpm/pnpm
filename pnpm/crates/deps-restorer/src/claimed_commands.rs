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
/// A directory is keyed by its lexically normalized path. Keys that name
/// one directory on disk, such as a project reached through a symlink and
/// through its real path, are the same directory to
/// [`Self::claim_recorded`] and [`Self::prune_stale_bins`].
#[derive(Debug, Default, Clone)]
pub struct ClaimedCommands {
    by_dir: HashMap<PathBuf, HashSet<String>>,
    /// Each key by its [`real_dir`], built on first use and dropped when a
    /// key is added.
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

    /// Adds `commands` to a directory a link pass already claimed for,
    /// under its own key or under a key naming the same directory on disk.
    /// Returns `false`, recording nothing, when no pass claimed it.
    pub fn claim_recorded(&mut self, modules_dir: &Path, commands: HashSet<String>) -> bool {
        let key = lexical_normalize(modules_dir);
        let key = if self.by_dir.contains_key(&key) {
            key
        } else {
            let Some(alias) = self.keys_naming(modules_dir).first() else { return false };
            alias.clone()
        };
        self.by_dir
            .get_mut(&key)
            .is_some_and(|claimed| {
                claimed.extend(commands);
                true
            })
    }

    /// Removes from each `<modules_dir>/.bin` the shims no pass claimed for
    /// that directory. Directories naming one directory on disk are pruned
    /// once, against the claims recorded under all of them.
    pub fn prune_stale_bins<'a>(
        &self,
        modules_dirs: impl IntoIterator<Item = &'a Path>,
    ) -> Result<(), LinkBinsError> {
        let mut pruned = HashSet::new();
        for modules_dir in modules_dirs {
            let real = real_dir(modules_dir);
            if !pruned.insert(real.clone()) {
                continue;
            }
            let claimed: HashSet<String> = self
                .keys_naming(modules_dir)
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

    fn keys_naming(&self, modules_dir: &Path) -> &[PathBuf] {
        self.keys_by_real_dir
            .get_or_init(|| {
                let mut keys: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
                for key in self.by_dir.keys() {
                    keys.entry(real_dir(key))
                        .or_default()
                        .push(key.clone());
                }
                keys
            })
            .get(&real_dir(modules_dir))
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
