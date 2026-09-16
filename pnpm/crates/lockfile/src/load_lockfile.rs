use crate::{
    EnvLockfile, Lockfile, ProjectSnapshot, extract_env_document, extract_main_document,
    git_merge_file::{MERGE_CONFLICT_OURS, ParsedWantedFile, parse_wanted_file},
    load_lockfile::repair_document::prepare_value_for_fix,
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

/// Whether the combined lockfile's leading env document was left
/// conflicted by Git *and* merges cleanly.
///
/// The merge has to be attempted, not guessed at from the markers:
/// counting a conflict the merge cannot resolve would have the install
/// report a merge that never happened. A marker inside a YAML comment or
/// scalar is ruled out the same way, by the document parsing as it
/// stands.
///
/// The substring test only skips that work for the documents that
/// plainly carry no marker, which is all of them but the conflicted few.
fn env_document_merges(content: &str, file_path: &Path) -> bool {
    let Some(env) = extract_env_document(content) else { return false };
    if !env.contains(MERGE_CONFLICT_OURS) {
        return false;
    }
    EnvLockfile::parse_conflicted_document(&env, file_path)
        .is_ok_and(|parsed| parsed.merged_conflict_files > 0)
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
        Ok(Self::load_wanted_from_path(&dir.join(Lockfile::FILE_NAME))?.value)
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
        Ok(Self::load_wanted_detailed(dir, selection)?.lockfile
            .map(|lockfile| Arc::try_unwrap(lockfile).unwrap_or_else(|shared| (*shared).clone())))
    }

    /// [`Self::load_wanted`] keeping the importers the fold started from.
    pub fn load_wanted_detailed(
        dir: &Path,
        selection: &WantedLockfileSelection,
    ) -> Result<LoadedWantedLockfile, LoadLockfileError> {
        for file_name in selection.read_order() {
            let path = dir.join(file_name);
            let parsed = Self::load_wanted_from_path(&path)?;
            let Some(lockfile) = parsed.value else { continue };
            return if selection.merge_git_branch_lockfiles {
                let pre_merge_importers = lockfile.importers.clone();
                let (merged, branch_conflict_files) = merge_git_branch_lockfiles(lockfile, dir)?;
                Ok(LoadedWantedLockfile {
                    lockfile: Some(Arc::new(merged)),
                    pre_merge_importers: Some(pre_merge_importers),
                    merged_conflict_files: parsed.merged_conflict_files + branch_conflict_files,
                })
            } else {
                Ok(LoadedWantedLockfile {
                    lockfile: Some(Arc::new(lockfile)),
                    pre_merge_importers: None,
                    merged_conflict_files: parsed.merged_conflict_files,
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
            let parsed = Self::load_repair_views_from_path(&path)?;
            let Some(views) = parsed.value else { continue };
            return if selection.merge_git_branch_lockfiles {
                let pre_merge_importers = views.seed.importers.clone();
                let (views, branch_conflict_files) = merge_git_branch_lockfile_repairs(views, dir)?;
                Ok(LoadedRepairLockfile {
                    views: Some(views),
                    pre_merge_importers: Some(pre_merge_importers),
                    merged_conflict_files: parsed.merged_conflict_files + branch_conflict_files,
                })
            } else {
                Ok(LoadedRepairLockfile {
                    views: Some(views),
                    pre_merge_importers: None,
                    merged_conflict_files: parsed.merged_conflict_files,
                })
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

    /// [`Self::load_from_path`] for the file an install reads and writes
    /// back, so a Git-conflicted one is merged rather than discarded.
    fn load_wanted_from_path(
        file_path: &Path,
    ) -> Result<ParsedWantedFile<Self>, LoadLockfileError> {
        let Some(content) = read_lockfile_text(file_path)? else {
            return Ok(ParsedWantedFile { value: None, merged_conflict_files: 0 });
        };
        let mut parsed =
            parse_wanted_file(&content, file_path, Self::parse, merge_lockfile_changes)?;
        // A conflict confined to the env document leaves the main one
        // parsing as it stands, so the recovery above never runs. The file
        // still holds markers, and only an install that writes it back
        // clears them, so it counts as conflicted either way.
        if parsed.merged_conflict_files == 0 && env_document_merges(&content, file_path) {
            parsed.merged_conflict_files = 1;
        }
        Ok(parsed)
    }

    /// [`Self::load_from_path`] deriving both repair views from the
    /// single read.
    fn load_repair_views_from_path(
        file_path: &Path,
    ) -> Result<ParsedWantedFile<RepairLockfileViews>, LoadLockfileError> {
        let Some(content) = read_lockfile_text(file_path)? else {
            return Ok(ParsedWantedFile { value: None, merged_conflict_files: 0 });
        };
        parse_wanted_file(
            &content,
            file_path,
            Self::parse_repair_views,
            merge_repair_lockfile_views,
        )
    }
}

mod repair_document;

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
    /// Number of lockfiles whose Git conflict markers were merged while loading.
    pub merged_conflict_files: usize,
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
    merged_conflict_files: usize,
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
        LoadedRepairLockfile {
            views,
            pre_merge_importers: loaded.pre_merge_importers,
            merged_conflict_files: loaded.merged_conflict_files,
        }
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

    pub(crate) fn merged_conflict_files(&self) -> usize {
        self.merged_conflict_files
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

fn merge_repair_lockfile_views(
    ours: &RepairLockfileViews,
    theirs: &RepairLockfileViews,
) -> RepairLockfileViews {
    RepairLockfileViews {
        seed: merge_lockfile_changes(&ours.seed, &theirs.seed),
        merge: merge_lockfile_changes(&ours.merge, &theirs.merge),
    }
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
        branch_file
            .into_iter()
            .chain([Lockfile::FILE_NAME])
    }
}

fn merge_git_branch_lockfiles(
    base: Lockfile,
    dir: &Path,
) -> Result<(Lockfile, usize), LoadLockfileError> {
    let branch_lockfiles =
        Lockfile::git_branch_lockfiles(dir).map_err(LoadLockfileError::ReadFile)?;
    let mut merged = base;
    let mut merged_conflict_files = 0;
    for path in branch_lockfiles {
        let parsed = Lockfile::load_wanted_from_path(&path)?;
        merged_conflict_files += parsed.merged_conflict_files;
        if let Some(branch_lockfile) = parsed.value {
            merged = merge_lockfile_changes(&merged, &branch_lockfile);
        }
    }
    Ok((merged, merged_conflict_files))
}

/// [`merge_git_branch_lockfiles`] folding each branch lockfile into both
/// repair views, so a branch file too is read once for the pair.
fn merge_git_branch_lockfile_repairs(
    mut base: RepairLockfileViews,
    dir: &Path,
) -> Result<(RepairLockfileViews, usize), LoadLockfileError> {
    let branch_lockfiles =
        Lockfile::git_branch_lockfiles(dir).map_err(LoadLockfileError::ReadFile)?;
    let mut merged_conflict_files = 0;
    for path in branch_lockfiles {
        let parsed = Lockfile::load_repair_views_from_path(&path)?;
        merged_conflict_files += parsed.merged_conflict_files;
        if let Some(branch) = parsed.value {
            base = merge_repair_lockfile_views(&base, &branch);
        }
    }
    Ok((base, merged_conflict_files))
}
