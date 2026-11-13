//! How often parts of the workspace change in the same commit, read from
//! `git log`.

use crate::tree::git;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::Path,
};

/// Starts each commit's record in the `git log` output that [`read_log`]
/// asks for. A control character, so `git` quotes it in a path.
const RECORD_START: char = '\u{1e}';

/// The co-change counts of the commits in one `git log` output.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CoChange {
    /// The commits read, whether or not they touched a unit.
    pub commits: usize,
    /// The commit dates of the oldest and the newest commit read.
    pub span: Option<(String, String)>,
    /// How many commits touched each unit.
    pub changes: BTreeMap<String, usize>,
    /// How many commits touched both units of a pair, keyed by the pair in
    /// name order. Wide commits are left out.
    pub pairs: BTreeMap<(String, String), usize>,
    /// The most units a commit may touch and still count for pairs.
    pub max_units: usize,
    /// The commits that touched more than `max_units` units.
    pub wide_commits: usize,
}

impl CoChange {
    /// Counts the output of [`read_log`]. `unit_of` maps a changed path to
    /// the unit that contains it, such as a crate, or to `None` to leave
    /// the path out. A commit that touches more than `max_units` units, such
    /// as a rename across the workspace, counts for each unit but for no pair.
    pub fn count(log: &str, unit_of: impl Fn(&str) -> Option<String>, max_units: usize) -> Self {
        let mut co_change = Self { max_units, ..Self::default() };
        let mut dates = Vec::new();
        for record in log.split(RECORD_START).skip(1) {
            let mut lines = record.lines();
            dates.extend(lines.next().map(str::to_owned));
            let units = lines.filter_map(&unit_of).collect::<BTreeSet<_>>();
            co_change.add_commit(&units);
        }
        co_change.span = dates
            .last()
            .cloned()
            .zip(dates.first().cloned());
        co_change
    }

    fn add_commit(&mut self, units: &BTreeSet<String>) {
        self.commits += 1;
        for unit in units {
            *self.changes.entry(unit.clone()).or_default() += 1;
        }
        if units.len() > self.max_units {
            self.wide_commits += 1;
            return;
        }
        for (index, unit) in units.iter().enumerate() {
            for other in units.iter().skip(index + 1) {
                *self.pairs
                    .entry((unit.clone(), other.clone()))
                    .or_default() += 1;
            }
        }
    }

    /// How many commits touched both units, in either order.
    pub fn together(&self, left: &str, right: &str) -> usize {
        let key = if left <= right { (left, right) } else { (right, left) };
        self.pairs
            .get(&(key.0.to_owned(), key.1.to_owned()))
            .copied()
            .unwrap_or_default()
    }

    pub fn changes_of(&self, unit: &str) -> usize {
        self.changes
            .get(unit)
            .copied()
            .unwrap_or_default()
    }
}

/// The paths each of the latest `commits` non-merge commits before `HEAD`
/// changed, with each commit's date, in the form [`CoChange::count`] reads.
/// A move counts as a change to both paths. In a shallow clone, the commits
/// at the shallow boundary are left out: `git` lists every file of a commit
/// without parents as changed.
pub fn read_log(root: &Path, commits: usize) -> Result<String, String> {
    let max_count = format!("--max-count={commits}");
    let format = format!("--format={RECORD_START}%cs");
    let mut args = ["log", "--no-merges", "--no-renames", "--name-only", &max_count, &format]
        .map(str::to_owned)
        .to_vec();
    args.push("HEAD".to_owned());
    args.extend(
        shallow_boundary(root)?
            .into_iter()
            .map(|commit| format!("^{commit}")),
    );
    let args = args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    git(root, &args)
}

/// The commits whose parents a shallow clone lacks, none in a full clone.
fn shallow_boundary(root: &Path) -> Result<Vec<String>, String> {
    let path = git(root, &["rev-parse", "--git-path", "shallow"])?;
    let path = root.join(path.trim_end());
    match fs::read_to_string(&path) {
        Ok(text) => Ok(text
            .lines()
            .map(str::to_owned)
            .collect()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("read {path:?}: {error}")),
    }
}

/// Maps a path to the crate whose directory contains it.
pub fn crate_of(dirs: &BTreeMap<String, String>) -> impl Fn(&str) -> Option<String> {
    let prefixes = dirs
        .iter()
        .map(|(name, dir)| (format!("{dir}/"), name.clone()))
        .collect::<Vec<_>>();
    move |path| {
        prefixes
            .iter()
            .filter(|(prefix, _)| path.starts_with(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len())
            .map(|(_, name)| name.clone())
    }
}

/// Maps a path in the crate whose manifest is in `crate_dir` to the
/// top-level module whose file or directory under `src/` contains it.
/// The crate root files map to `None`.
pub fn module_of(crate_dir: &str) -> impl Fn(&str) -> Option<String> {
    let src = format!("{crate_dir}/src/");
    move |path| {
        let relative = path.strip_prefix(src.as_str())?;
        let first = relative.split('/').next()?;
        let is_file = first.len() == relative.len();
        let name = if is_file { first.strip_suffix(".rs")? } else { first };
        let is_root = is_file && matches!(name, "lib" | "main");
        (!is_root).then(|| name.to_owned())
    }
}
