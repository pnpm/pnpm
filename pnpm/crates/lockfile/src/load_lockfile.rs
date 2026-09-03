use crate::{
    Lockfile, LockfileResolution, ProjectSnapshot, SnapshotEntry, extract_main_document,
    merge_lockfile_changes,
};
use derive_more::{Display, Error};
use pipe_trait::Pipe;
use pnpm_diagnostics::miette::{self, Diagnostic};
use serde_saphyr::MessageFormatter;
use std::{
    collections::HashMap,
    env, fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
    sync::Arc,
};

const DEFAULT_YAML_MAX_EVENTS: usize = 1_000_000;
const DEFAULT_YAML_MAX_NODES: usize = 250_000;
const DEFAULT_YAML_MAX_SCALAR_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_YAML_MAX_READER_INPUT_BYTES: usize = 256 * 1024 * 1024;

/// Error when reading lockfile the filesystem.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum LoadLockfileError {
    #[display("Failed to get current_dir: {_0}")]
    #[diagnostic(code(ERR_PNPM_LOCKFILE_CURRENT_DIR))]
    CurrentDir(io::Error),

    #[display("Failed to read lockfile content: {_0}")]
    #[diagnostic(code(ERR_PNPM_LOCKFILE_READ_FILE))]
    ReadFile(io::Error),

    #[display(
        "The lockfile at \"{}\" is broken: {}",
        path.display(),
        reason
    )]
    #[diagnostic(code(ERR_PNPM_BROKEN_LOCKFILE))]
    ParseYaml { path: PathBuf, reason: String },
}

impl LoadLockfileError {
    pub(super) fn parse_yaml(path: &Path, source: &serde_saphyr::Error) -> Self {
        Self::ParseYaml { path: path.to_path_buf(), reason: format_yaml_error(source) }
    }
}

fn format_yaml_error(error: &serde_saphyr::Error) -> String {
    let error = error.without_snippet();
    let reason = serde_saphyr::DefaultMessageFormatter.format_message(error);
    if let Some(location) = error.location() {
        format!("{reason} ({}:{})", location.line(), location.column())
    } else {
        reason.into_owned()
    }
}

/// The parsing budgets for a lockfile document of `document_len` bytes.
///
/// Every size-proportional budget is raised to the document's byte length:
/// none of these dimensions can exceed the size of an input that is already
/// in memory, so a valid lockfile must never trip them, however large. The
/// remaining defaults (aliases, anchors, depth, documents) bound YAML shapes
/// the lockfile emitter never produces and stay as security caps.
fn yaml_parse_options(document_len: usize) -> serde_saphyr::Options {
    serde_saphyr::options! {
        budget: serde_saphyr::budget! {
            max_events: document_len.max(DEFAULT_YAML_MAX_EVENTS),
            max_nodes: document_len.max(DEFAULT_YAML_MAX_NODES),
            max_total_scalar_bytes: document_len.max(DEFAULT_YAML_MAX_SCALAR_BYTES),
            max_total_comment_bytes: document_len.max(DEFAULT_YAML_MAX_SCALAR_BYTES),
            max_reader_input_bytes: Some(document_len.max(DEFAULT_YAML_MAX_READER_INPUT_BYTES)),
        },
    }
}

/// The text of a lockfile file, or `None` when it is absent. Every other
/// read failure is an error: an existing-but-unreadable lockfile must not
/// be mistaken for a missing one.
fn read_lockfile_text(file_path: &Path) -> Result<Option<String>, LoadLockfileError> {
    match fs::read_to_string(file_path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => error.pipe(LoadLockfileError::ReadFile).pipe(Err),
    }
}

impl Lockfile {
    /// Load lockfile from the current directory.
    pub fn load_from_current_dir() -> Result<Option<Self>, LoadLockfileError> {
        let file_path =
            env::current_dir().map_err(LoadLockfileError::CurrentDir)?.join(Lockfile::FILE_NAME);
        Self::load_from_path(&file_path)
    }

    /// Load the *current* lockfile from
    /// `<virtual_store_dir>/lock.yaml`: the file records what pacquet
    /// actually materialized on the previous install and is diffed
    /// against the wanted lockfile to decide which snapshots can be
    /// skipped.
    ///
    /// Returns `Ok(None)` when the file is absent (a fresh install
    /// against an empty `node_modules`), treating ENOENT as `null`.
    /// Same parse / version-check path as the wanted lockfile, so a
    /// major-version mismatch surfaces as a parse error rather than
    /// silently dropping the file.
    pub fn load_current_from_virtual_store_dir(
        virtual_store_dir: &Path,
    ) -> Result<Option<Self>, LoadLockfileError> {
        let file_path = virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME);
        Self::load_from_path(&file_path)
    }

    /// Load the wanted lockfile (`<dir>/pnpm-lock.yaml`) — a
    /// directory-addressed loader for callers that resolve into a
    /// directory other than the process's current one. Returns
    /// `Ok(None)` when the file is absent, same as
    /// [`Self::load_from_current_dir`].
    pub fn load_wanted_from_dir(dir: &Path) -> Result<Option<Self>, LoadLockfileError> {
        Self::load_from_path(&dir.join(Lockfile::FILE_NAME))
    }

    /// Load the wanted lockfile an install reads, honoring the per-branch
    /// lockfile settings.
    ///
    /// A branch-suffixed selection falls back to `pnpm-lock.yaml` when the
    /// branch has no lockfile of its own yet, so the first install on a
    /// new branch starts from the shared resolution rather than from
    /// nothing. Under `mergeGitBranchLockfiles` every branch lockfile in
    /// `dir` is then folded into whichever file was read.
    pub fn load_wanted(
        dir: &Path,
        selection: &WantedLockfileSelection,
    ) -> Result<Option<Self>, LoadLockfileError> {
        Ok(Self::load_wanted_detailed(dir, selection)?
            .lockfile
            .map(|lockfile| Arc::try_unwrap(lockfile).unwrap_or_else(|shared| (*shared).clone())))
    }

    /// [`Self::load_wanted`] keeping the importers the fold started from.
    pub fn load_wanted_detailed(
        dir: &Path,
        selection: &WantedLockfileSelection,
    ) -> Result<LoadedWantedLockfile, LoadLockfileError> {
        for file_name in selection.read_order() {
            let path = dir.join(file_name);
            let Some(lockfile) = Self::load_from_path(&path)? else { continue };
            return if selection.merge_git_branch_lockfiles {
                let pre_merge_importers = lockfile.importers.clone();
                let merged = merge_git_branch_lockfiles(lockfile, dir)?;
                Ok(LoadedWantedLockfile {
                    lockfile: Some(Arc::new(merged)),
                    pre_merge_importers: Some(pre_merge_importers),
                })
            } else {
                Ok(LoadedWantedLockfile {
                    lockfile: Some(Arc::new(lockfile)),
                    pre_merge_importers: None,
                })
            };
        }
        Ok(LoadedWantedLockfile::default())
    }

    /// [`Self::load_wanted_detailed`] for a repairing install: each file
    /// it reads is read once and yields both of the views the repair
    /// needs. See [`LoadedRepairLockfile`] for why they must be paired.
    pub(crate) fn load_wanted_detailed_for_fix(
        dir: &Path,
        selection: &WantedLockfileSelection,
    ) -> Result<LoadedRepairLockfile, LoadLockfileError> {
        for file_name in selection.read_order() {
            let path = dir.join(file_name);
            let Some(views) = Self::load_repair_views_from_path(&path)? else { continue };
            return if selection.merge_git_branch_lockfiles {
                let pre_merge_importers = views.seed.importers.clone();
                let views = merge_git_branch_lockfile_repairs(views, dir)?;
                Ok(LoadedRepairLockfile {
                    views: Some(views),
                    pre_merge_importers: Some(pre_merge_importers),
                })
            } else {
                Ok(LoadedRepairLockfile { views: Some(views), pre_merge_importers: None })
            };
        }
        Ok(LoadedRepairLockfile::default())
    }

    /// Whether `<dir>/pnpm-lock.yaml` would load as `Some`: the file
    /// exists and its main document is non-empty. The same absence
    /// rules as [`Self::load_wanted_from_dir`] (a missing file, an
    /// empty file, and an env-only combined document all count as
    /// absent) without paying for the YAML parse — only the read and
    /// the document split.
    ///
    /// Any read failure other than `NotFound` (permissions, invalid
    /// UTF-8, I/O) reports the file as present: an existing-but-
    /// unreadable lockfile must not be mistaken for a missing one —
    /// the regenerate-on-missing path would overwrite it — and the
    /// real load surfaces the underlying error when the contents are
    /// actually needed.
    #[must_use]
    pub fn wanted_exists_in_dir(dir: &Path) -> bool {
        Self::wanted_exists(dir, Lockfile::FILE_NAME)
    }

    /// [`Self::wanted_exists_in_dir`] for a caller-chosen file name.
    ///
    /// Deliberately no fallback to `pnpm-lock.yaml`: pnpm's
    /// `existsNonEmptyWantedLockfile` asks about the one file the install
    /// would write, so a branch that has not been installed on yet reads
    /// as having no lockfile even when the shared one is on disk.
    #[must_use]
    pub fn wanted_exists(dir: &Path, file_name: &str) -> bool {
        match fs::read_to_string(dir.join(file_name)) {
            Ok(content) => !extract_main_document(&content).trim().is_empty(),
            Err(error) => error.kind() != ErrorKind::NotFound,
        }
    }

    /// Parse lockfile text that was read from `file_path` — the path is
    /// only used to name the file in a parse error. Returns `Ok(None)`
    /// for the same empty-document cases as
    /// [`Self::load_wanted_from_dir`], so a caller holding an in-memory
    /// snapshot of the file gets the same value the loader would.
    pub fn parse(content: &str, file_path: &Path) -> Result<Option<Self>, LoadLockfileError> {
        let main = extract_main_document(content);
        if main.trim().is_empty() {
            return Ok(None);
        }
        serde_saphyr::from_str_with_options::<Self>(&main, yaml_parse_options(main.len()))
            .map(|mut lockfile| {
                lockfile.reconstruct_missing_directory_resolutions();
                Some(lockfile)
            })
            .map_err(|source| LoadLockfileError::parse_yaml(file_path, &source))
    }

    fn parse_repair_views(
        content: &str,
        file_path: &Path,
    ) -> Result<Option<RepairLockfileViews>, LoadLockfileError> {
        let main = extract_main_document(content);
        if main.trim().is_empty() {
            return Ok(None);
        }
        let mut value = serde_saphyr::from_str_with_options::<serde_json::Value>(
            &main,
            yaml_parse_options(main.len()),
        )
        .map_err(|source| LoadLockfileError::parse_yaml(file_path, &source))?;
        prepare_value_for_fix(&mut value);
        serde_json::from_value::<Self>(value)
            .map(|mut merge| {
                merge.reconstruct_missing_directory_resolutions();
                let mut seed = merge.clone();
                seed.prepare_for_fix();
                Some(RepairLockfileViews { seed, merge })
            })
            .map_err(|source| LoadLockfileError::ParseYaml {
                path: file_path.to_path_buf(),
                reason: source.to_string(),
            })
    }

    /// Load a lockfile from an explicit path. Returns `Ok(None)` when the
    /// file is absent or its main document is empty, the same absence
    /// rules the directory-addressed loaders use.
    pub fn load_from_path(file_path: &Path) -> Result<Option<Self>, LoadLockfileError> {
        let Some(content) = read_lockfile_text(file_path)? else { return Ok(None) };
        Self::parse(&content, file_path)
    }

    /// [`Self::load_from_path`] deriving both repair views from the
    /// single read.
    fn load_repair_views_from_path(
        file_path: &Path,
    ) -> Result<Option<RepairLockfileViews>, LoadLockfileError> {
        let Some(content) = read_lockfile_text(file_path)? else { return Ok(None) };
        Self::parse_repair_views(&content, file_path)
    }
}

#[cfg(test)]
mod tests;

/// A wanted lockfile as it was loaded, with the importers as they stood
/// before `mergeGitBranchLockfiles` folded the branch lockfiles in.
///
/// Separating an entry the fold introduced from one the read file already
/// carried needs that "before": the merged lockfile no longer holds it,
/// and only the fold's own additions may be reconciled away against the
/// manifests. `pre_merge_importers` is `None` when no fold was attempted.
#[derive(Debug, Default, Clone)]
pub struct LoadedWantedLockfile {
    /// Behind an `Arc` so a consumer that seeds long-lived machinery
    /// (the resolver's lockfile-reuse pass above all) can share the
    /// parsed document instead of deep-copying a workspace-scale
    /// lockfile.
    pub lockfile: Option<Arc<Lockfile>>,
    pub pre_merge_importers: Option<HashMap<String, ProjectSnapshot>>,
}

/// The views a repairing install reads the wanted lockfile for, and the
/// importers the branch-lockfile fold started from — see
/// [`LoadedWantedLockfile`] for why the caller needs those.
///
/// The views are held as one value because they are one file generation
/// seen two ways: caching them separately lets a repair resolve from one
/// generation and restore the projects outside its filter from another.
#[derive(Debug, Default)]
pub(crate) struct LoadedRepairLockfile {
    views: Option<RepairLockfileViews>,
    pre_merge_importers: Option<HashMap<String, ProjectSnapshot>>,
}

impl LoadedRepairLockfile {
    /// Derive both views from a lockfile that is already in memory,
    /// for the sources [`Lockfile::load_wanted_detailed_for_fix`] never
    /// reads.
    pub(crate) fn from_loaded(loaded: LoadedWantedLockfile) -> Self {
        let views = loaded.lockfile.map(|merge| {
            let merge = Arc::try_unwrap(merge).unwrap_or_else(|shared| (*shared).clone());
            let mut seed = merge.clone();
            seed.prepare_for_fix();
            RepairLockfileViews { seed, merge }
        });
        LoadedRepairLockfile { views, pre_merge_importers: loaded.pre_merge_importers }
    }

    pub(crate) fn seed(&self) -> Option<&Lockfile> {
        self.views.as_ref().map(|views| &views.seed)
    }

    pub(crate) fn merge(&self) -> Option<&Lockfile> {
        self.views.as_ref().map(|views| &views.merge)
    }

    pub(crate) fn pre_merge_importers(&self) -> Option<&HashMap<String, ProjectSnapshot>> {
        self.pre_merge_importers.as_ref()
    }
}

/// One wanted-lockfile generation seen two ways: `seed` has the fields a
/// repairing resolution regenerates cleared, `merge` is intact so the
/// projects a filtered repair did not select keep what they had.
#[derive(Debug)]
struct RepairLockfileViews {
    seed: Lockfile,
    merge: Lockfile,
}

/// Which wanted-lockfile file an install reads and writes, and whether the
/// other branches' lockfiles are folded into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WantedLockfileSelection {
    /// The file the install reads first and writes back:
    /// `pnpm-lock.yaml`, or the `pnpm-lock.<branch>.yaml` that
    /// `useGitBranchLockfile` picked.
    pub file_name: String,
    /// `mergeGitBranchLockfiles`: fold every `pnpm-lock.<branch>.yaml`
    /// next to the file that was read into the loaded lockfile. The
    /// install deletes them once it has written the merge back.
    pub merge_git_branch_lockfiles: bool,
}

impl Default for WantedLockfileSelection {
    fn default() -> Self {
        WantedLockfileSelection {
            file_name: Lockfile::FILE_NAME.to_owned(),
            merge_git_branch_lockfiles: false,
        }
    }
}

impl WantedLockfileSelection {
    /// The file names to try, most specific first.
    fn read_order(&self) -> impl Iterator<Item = &str> {
        let branch_file = (self.file_name != Lockfile::FILE_NAME).then_some(&*self.file_name);
        branch_file.into_iter().chain([Lockfile::FILE_NAME])
    }
}

fn merge_git_branch_lockfiles(base: Lockfile, dir: &Path) -> Result<Lockfile, LoadLockfileError> {
    let branch_lockfiles =
        Lockfile::git_branch_lockfiles(dir).map_err(LoadLockfileError::ReadFile)?;
    let mut merged = base;
    for path in branch_lockfiles {
        if let Some(branch_lockfile) = Lockfile::load_from_path(&path)? {
            merged = merge_lockfile_changes(&merged, &branch_lockfile);
        }
    }
    Ok(merged)
}

/// [`merge_git_branch_lockfiles`] folding each branch lockfile into both
/// repair views, so a branch file too is read once for the pair.
fn merge_git_branch_lockfile_repairs(
    mut base: RepairLockfileViews,
    dir: &Path,
) -> Result<RepairLockfileViews, LoadLockfileError> {
    let branch_lockfiles =
        Lockfile::git_branch_lockfiles(dir).map_err(LoadLockfileError::ReadFile)?;
    for path in branch_lockfiles {
        if let Some(branch) = Lockfile::load_repair_views_from_path(&path)? {
            base.seed = merge_lockfile_changes(&base.seed, &branch.seed);
            base.merge = merge_lockfile_changes(&base.merge, &branch.merge);
        }
    }
    Ok(base)
}

fn prepare_value_for_fix(value: &mut serde_json::Value) {
    let Some(root) = value.as_object_mut() else { return };
    for key in [
        "settings",
        "catalogs",
        "overrides",
        "packageExtensionsChecksum",
        "pnpmfileChecksum",
        "ignoredOptionalDependencies",
        "patchedDependencies",
        "time",
    ] {
        discard_invalid_generated_field(root, key);
    }
    if let Some(packages) = root.get_mut("packages").and_then(serde_json::Value::as_object_mut) {
        packages.retain(|_, metadata| {
            if serde_json::from_value::<crate::PackageMetadata>(metadata.clone()).is_ok() {
                return true;
            }
            let Some(resolution) = metadata.get("resolution").cloned() else { return false };
            if serde_json::from_value::<LockfileResolution>(resolution.clone()).is_err() {
                return false;
            }
            *metadata = serde_json::json!({ "resolution": resolution });
            true
        });
    }
    if let Some(snapshots) = root.get_mut("snapshots").and_then(serde_json::Value::as_object_mut) {
        for snapshot in snapshots.values_mut() {
            if serde_json::from_value::<SnapshotEntry>(snapshot.clone()).is_ok() {
                continue;
            }
            let dependencies = snapshot.get("dependencies").cloned();
            let optional_dependencies = snapshot.get("optionalDependencies").cloned();
            let mut retained = serde_json::Map::new();
            if let Some(dependencies) = dependencies {
                retained.insert("dependencies".to_string(), dependencies);
            }
            if let Some(optional_dependencies) = optional_dependencies {
                retained.insert("optionalDependencies".to_string(), optional_dependencies);
            }
            let candidate = serde_json::Value::Object(retained);
            *snapshot = if serde_json::from_value::<SnapshotEntry>(candidate.clone()).is_ok() {
                candidate
            } else {
                serde_json::json!({})
            };
        }
    }
}

fn discard_invalid_generated_field(
    root: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
) {
    let Some(value) = root.get(key).cloned() else { return };
    let mut candidate = serde_json::Map::from_iter([
        ("lockfileVersion".to_owned(), serde_json::json!("9.0")),
        ("importers".to_owned(), serde_json::json!({})),
    ]);
    candidate.insert(key.to_owned(), value);
    if serde_json::from_value::<Lockfile>(serde_json::Value::Object(candidate)).is_err() {
        root.remove(key);
    }
}
