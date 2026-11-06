use pnpm_cmd_shim::{LinkBinsError, prune_stale_bins};
use pnpm_fs::lexical_normalize;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

/// The commands the link passes of one install shimmed into each modules
/// directory's `.bin`. Both linkers record their claims here, and a shim no
/// pass claimed is stale.
///
/// A directory is keyed by its lexically normalized path. A key may also
/// name it through the real path of its parent, the way a project reached
/// through a symlink records it. [`Self::claim_recorded`] and
/// [`Self::prune_stale_bins`] treat both keys as the one directory.
#[derive(Debug, Default, Clone)]
pub struct ClaimedCommands(HashMap<PathBuf, HashSet<String>>);

impl ClaimedCommands {
    pub fn claim(&mut self, modules_dir: &Path, commands: impl IntoIterator<Item = String>) {
        self.commands_mut(modules_dir).extend(commands);
    }

    /// The commands claimed for `modules_dir`, recording the directory with
    /// no commands when nothing claimed it yet.
    pub fn commands_mut(&mut self, modules_dir: &Path) -> &mut HashSet<String> {
        self.0
            .entry(lexical_normalize(modules_dir))
            .or_default()
    }

    /// The commands claimed under `modules_dir` itself.
    #[must_use]
    pub fn get(&self, modules_dir: &Path) -> Option<&HashSet<String>> {
        self.0.get(&lexical_normalize(modules_dir))
    }

    pub fn merge(&mut self, other: Self) {
        for (modules_dir, commands) in other.0 {
            self.claim(&modules_dir, commands);
        }
    }

    /// Adds `commands` to a directory a link pass already claimed, under
    /// the first of its keys recorded. Returns `false`, recording
    /// nothing, when no pass claimed it.
    pub fn claim_recorded(&mut self, modules_dir: &Path, commands: HashSet<String>) -> bool {
        let Some(key) = keys(modules_dir).find(|key| self.0.contains_key(key)) else {
            return false;
        };
        self.commands_mut(&key).extend(commands);
        true
    }

    /// Removes from each `<modules_dir>/.bin` the shims claimed under none
    /// of the directory's keys. A directory named twice is pruned once.
    pub fn prune_stale_bins<'a>(
        &self,
        modules_dirs: impl IntoIterator<Item = &'a Path>,
    ) -> Result<(), LinkBinsError> {
        let mut pruned = HashSet::new();
        for modules_dir in modules_dirs {
            let dir_keys: Vec<PathBuf> = keys(modules_dir).collect();
            let physical = dir_keys.last().expect("a directory has at least one key");
            if !pruned.insert(physical.clone()) {
                continue;
            }
            let claimed: HashSet<String> = dir_keys
                .iter()
                .filter_map(|key| self.0.get(key))
                .flatten()
                .cloned()
                .collect();
            prune_stale_bins(&physical.join(".bin"), &claimed)?;
        }
        Ok(())
    }

    /// Every recorded directory, in no particular order.
    pub fn dirs(&self) -> impl Iterator<Item = &Path> {
        self.0.keys().map(PathBuf::as_path)
    }
}

/// The keys `modules_dir` may be recorded under: its normalized path, then,
/// when its parent exists and resolves elsewhere, the parent's real path
/// joined with its name.
fn keys(modules_dir: &Path) -> impl Iterator<Item = PathBuf> {
    let lexical = lexical_normalize(modules_dir);
    let real = modules_dir
        .parent()
        .zip(modules_dir.file_name())
        .and_then(|(parent, name)| {
            std::fs::canonicalize(parent)
                .ok()
                .map(|real_parent| lexical_normalize(&real_parent.join(name)))
        })
        .filter(|real| *real != lexical);
    std::iter::once(lexical).chain(real)
}

#[cfg(test)]
mod tests;
