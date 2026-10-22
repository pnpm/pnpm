//! What the last validation proved about each project, and the content
//! digests that decide whether an input changed since.
//!
//! A project is proven installed while its manifest and the lockfile still
//! have the digests recorded for it. Content digests, unlike mtimes compared
//! against a recorded timestamp, see an edit that kept its mtime, and a
//! rewrite that kept the content does not read as a change. A filtered
//! install vouches only for the projects it installed, so the records of the
//! others survive it unchanged.

use super::{Config, Lockfile, PackageManifest, Path, PathBuf};
use pnpm_workspace::importer_id_from_root_dir;
use pnpm_workspace_state::{FileSignature, Validation, WorkspaceState};
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs, io,
};

/// How many hex digits of a SHA-256 digest a record keeps.
const DIGEST_LEN: usize = 32;

/// A filesystem that keeps whole seconds may round an mtime down by up to
/// this much (FAT rounds to two seconds).
const WHOLE_SECOND_GRANULARITY_NS: i64 = 2_000_000_000;

/// The digest of the manifest the install read, which may not be the file on
/// disk: `add` writes its edit after the install. The parsed value is hashed
/// directly, so a manifest that only reorders its keys or its whitespace keeps
/// its digest.
pub(crate) fn manifest_digest(manifest: &PackageManifest) -> String {
    let mut hasher = Sha256::new();
    hash_value(&mut hasher, manifest.value());
    let mut digest = format!("{:x}", hasher.finalize());
    digest.truncate(DIGEST_LEN);
    digest
}

/// Feed `value` into `hasher` unambiguously: each node is tagged by its kind,
/// each string and container by its length, and object keys in sorted order.
fn hash_value(hasher: &mut Sha256, value: &serde_json::Value) {
    use serde_json::Value;
    let length = |hasher: &mut Sha256, length: usize| hasher.update((length as u64).to_le_bytes());
    let string = |hasher: &mut Sha256, text: &str| {
        length(hasher, text.len());
        hasher.update(text.as_bytes());
    };
    match value {
        Value::Null => hasher.update(b"n"),
        Value::Bool(true) => hasher.update(b"t"),
        Value::Bool(false) => hasher.update(b"f"),
        Value::Number(number) => {
            hasher.update(b"#");
            string(hasher, &number.to_string());
        }
        Value::String(text) => {
            hasher.update(b"s");
            string(hasher, text);
        }
        Value::Array(items) => {
            hasher.update(b"[");
            length(hasher, items.len());
            for item in items {
                hash_value(hasher, item);
            }
        }
        Value::Object(entries) => {
            hasher.update(b"{");
            length(hasher, entries.len());
            let mut sorted = entries.iter().collect::<Vec<_>>();
            sorted.sort_unstable_by_key(|(key, _)| *key);
            for (key, item) in sorted {
                string(hasher, key);
                hash_value(hasher, item);
            }
        }
    }
}

/// The file whose digest every project record names: `pnpm-lock.yaml`, or
/// the current lockfile standing in for it.
pub(crate) fn lockfile_paths(workspace_root: &Path, config: &Config) -> [PathBuf; 2] {
    [
        workspace_root.join(config.wanted_lockfile_name()),
        config.install_state_dir.join(Lockfile::CURRENT_FILE_NAME),
    ]
}

/// The state of an input file as the next check sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileDigest {
    Absent,
    /// The file exists but could not be read, so nothing about it is proven.
    Unreadable,
    Present(FileSignature),
}

impl FileDigest {
    pub(crate) fn digest(&self) -> Option<&str> {
        match self {
            Self::Present(signature) => Some(&signature.digest),
            Self::Absent | Self::Unreadable => None,
        }
    }
}

/// Reads input files, reusing a recorded digest wherever the file's stat
/// proves its content did not change.
pub(crate) struct FileDigests<'a> {
    recorded: Option<&'a Validation>,
}

impl<'a> FileDigests<'a> {
    pub(crate) fn new(recorded: Option<&'a Validation>) -> Self {
        Self { recorded }
    }

    pub(crate) fn of(&self, path: &Path) -> FileDigest {
        let metadata = match fs::metadata(path) {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => return FileDigest::Unreadable,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return FileDigest::Absent,
            Err(_) => return FileDigest::Unreadable,
        };
        let Some(stat) = FileStat::of(path, &metadata) else {
            return FileDigest::Unreadable;
        };
        if let Some(recorded) = self.recorded_signature(path)
            && stat.matches(recorded)
        {
            return FileDigest::Present(recorded.clone());
        }
        match pnpm_crypto_hash::create_hex_hash_from_file(path) {
            Ok(mut digest) => {
                digest.truncate(DIGEST_LEN);
                FileDigest::Present(FileSignature {
                    mtime_ns: stat.mtime_ns,
                    ctime_ns: stat.ctime_ns,
                    size: stat.size,
                    digest,
                })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => FileDigest::Absent,
            Err(_) => FileDigest::Unreadable,
        }
    }

    /// Whether the recorded signature of `path` stood for its content, so
    /// [`Self::of`] did not read the file.
    pub(crate) fn trusts(&self, path: &Path) -> bool {
        let Some(recorded) = self.recorded_signature(path) else {
            return false;
        };
        fs::metadata(path)
            .ok()
            .and_then(|metadata| FileStat::of(path, &metadata))
            .is_some_and(|stat| stat.matches(recorded))
    }

    /// The digest of the lockfile the project records name.
    pub(crate) fn lockfile(&self, workspace_root: &Path, config: &Config) -> (PathBuf, FileDigest) {
        let [wanted, current] = lockfile_paths(workspace_root, config);
        match self.of(&wanted) {
            FileDigest::Absent => {
                let digest = self.of(&current);
                (current, digest)
            }
            digest => (wanted, digest),
        }
    }

    /// The recorded signature of `path`, when its stat can be trusted to
    /// stand for its content: the file was last written before the recording
    /// stat'd it. One written in the same filesystem clock tick, or within the
    /// rounding of a whole-second filesystem, could have changed again without
    /// its stat moving.
    fn recorded_signature(&self, path: &Path) -> Option<&'a FileSignature> {
        let recorded = self.recorded?;
        let signature = recorded.files.get(&*path.to_string_lossy())?;
        signature_trusted(signature, recorded.recorded_at_ns).then_some(signature)
    }
}

/// Whether the next check trusts the stat of every file `validation` records.
pub(crate) fn trusts_every_file(validation: &Validation) -> bool {
    validation.files
        .values()
        .all(|signature| signature_trusted(signature, validation.recorded_at_ns))
}

fn signature_trusted(signature: &FileSignature, recorded_at_ns: i64) -> bool {
    let changed_ns = last_change_ns(signature.mtime_ns, signature.ctime_ns);
    let window = if changed_ns % 1_000_000_000 == 0 { WHOLE_SECOND_GRANULARITY_NS } else { 0 };
    changed_ns.saturating_add(window) < recorded_at_ns
}

/// The parts of a file's stat a [`FileSignature`] records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileStat {
    mtime_ns: i64,
    ctime_ns: Option<i64>,
    size: u64,
}

impl FileStat {
    /// `None` when the file cannot be stat'd.
    pub(crate) fn at(path: &Path) -> Option<Self> {
        Self::of(path, &fs::metadata(path).ok()?)
    }

    fn of(path: &Path, metadata: &fs::Metadata) -> Option<Self> {
        let elapsed = metadata
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        Some(Self {
            mtime_ns: i64::try_from(elapsed.as_nanos()).ok()?,
            ctime_ns: ctime_ns(path, metadata),
            size: metadata.len(),
        })
    }

    pub(crate) fn matches(&self, signature: &FileSignature) -> bool {
        self.mtime_ns == signature.mtime_ns
            && self.ctime_ns == signature.ctime_ns
            && self.size == signature.size
    }
}

fn last_change_ns(mtime_ns: i64, ctime_ns: Option<i64>) -> i64 {
    ctime_ns.map_or(mtime_ns, |ctime_ns| ctime_ns.max(mtime_ns))
}

/// The inode change time, which also moves when an edit restores the mtime.
#[cfg(unix)]
fn ctime_ns(_path: &Path, metadata: &fs::Metadata) -> Option<i64> {
    use std::os::unix::fs::MetadataExt as _;
    metadata
        .ctime()
        .checked_mul(1_000_000_000)?
        .checked_add(metadata.ctime_nsec())
}

/// NTFS's change time, which also moves when an edit restores the mtime. It
/// is what Node.js reports as `ctime` on Windows.
#[cfg(windows)]
fn ctime_ns(path: &Path, _metadata: &fs::Metadata) -> Option<i64> {
    use std::os::windows::{fs::OpenOptionsExt as _, io::AsRawHandle as _};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_BASIC_INFO, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FileBasicInfo, GetFileInformationByHandleEx,
    };
    /// 100-nanosecond intervals between 1601-01-01 and the Unix epoch.
    const UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;

    let file = fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(path)
        .ok()?;
    let mut info = std::mem::MaybeUninit::<FILE_BASIC_INFO>::uninit();
    // SAFETY: the handle stays open for the call, and `info` is a writable
    // buffer of exactly the size passed.
    let succeeded = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileBasicInfo,
            info.as_mut_ptr().cast(),
            u32::try_from(size_of::<FILE_BASIC_INFO>()).ok()?,
        )
    };
    if succeeded == 0 {
        return None;
    }
    // SAFETY: the call succeeded, so it filled `info`.
    let change_ticks = unsafe { info.assume_init() }.ChangeTime;
    change_ticks.checked_sub(UNIX_EPOCH_TICKS)?.checked_mul(100)
}

#[cfg(not(any(unix, windows)))]
fn ctime_ns(_path: &Path, _metadata: &fs::Metadata) -> Option<i64> {
    None
}

/// Which projects a state write vouches for.
pub(crate) enum Vouched<'a> {
    All,
    /// These project directories. Every other project keeps the record
    /// `previous` holds for it.
    Projects {
        dirs: &'a HashSet<PathBuf>,
        previous: Option<&'a WorkspaceState>,
    },
}

/// Everything [`record_validation`] reads besides the state it completes.
pub(crate) struct RecordValidation<'a> {
    pub(crate) workspace_root: &'a Path,
    pub(crate) config: &'a Config,
    pub(crate) project_manifests: &'a [(PathBuf, &'a PackageManifest)],
    pub(crate) vouched: &'a Vouched<'a>,
}

/// Record in `state` what this write proves, and mark it filtered when a
/// project is left unproven.
///
/// The files are stat'd before the filesystem clock is probed, so a file
/// rewritten after its stat either has a stat that no longer matches or falls
/// inside the window [`FileDigests`] distrusts.
///
/// Under `SOURCE_DATE_EPOCH`, whose output must not vary with file stats or
/// the clock, `recorded_at_ns` and each file signature's stat timestamps are
/// zeroed while keeping content digests. [`signature_trusted`] then always
/// returns `false`, so the next check rehashes the files on disk against the
/// recorded digests instead of trusting modification times.
pub(crate) fn record_validation(state: &mut WorkspaceState, inputs: &RecordValidation<'_>) {
    let previous = match inputs.vouched {
        Vouched::All => None,
        Vouched::Projects { previous, .. } => *previous,
    };
    let (lockfile, files) = recorded_files(state, inputs, previous);
    let Some(recorded_at_ns) = recorded_validation_time_ns(inputs) else {
        state.validation = None;
        state.filtered_install = true;
        return;
    };
    let shared_inputs = shared_input_paths(inputs.workspace_root, inputs.config, &state.pnpmfiles);
    let carried = previous
        .filter(|previous| shared_inputs_match(previous, state, &shared_inputs, &files))
        .and_then(|previous| previous.validation.as_ref());
    let mut validation =
        Validation { recorded_at_ns, files, lockfile: lockfile.clone(), ..Validation::default() };
    let recorded = record_projects(&mut validation, inputs, carried, lockfile.as_deref());
    state.filtered_install = recorded != inputs.project_manifests.len();
    state.validation = Some(validation);
}

fn recorded_validation_time_ns(inputs: &RecordValidation<'_>) -> Option<i64> {
    if inputs.config.source_date_epoch.is_some() {
        return Some(0);
    }
    super::filesystem_now_ns_in_modules(&inputs.config.workspace_state_modules_dir(
        inputs.workspace_root,
    ))
}

/// The lockfile digest and the signatures of the shared input files, read
/// the way the next check reads them.
fn recorded_files(
    state: &WorkspaceState,
    inputs: &RecordValidation<'_>,
    previous: Option<&WorkspaceState>,
) -> (Option<String>, BTreeMap<String, FileSignature>) {
    let digests = FileDigests::new(previous.and_then(|state| state.validation.as_ref()));
    let (lockfile_path, lockfile) = digests.lockfile(inputs.workspace_root, inputs.config);
    let shared_inputs = shared_input_paths(inputs.workspace_root, inputs.config, &state.pnpmfiles);
    let files = std::iter::once((lockfile_path, lockfile.clone()))
        .chain(
            shared_inputs
                .into_iter()
                .map(|path| {
                    let digest = digests.of(&path);
                    (path, digest)
                }),
        )
        .filter_map(|(path, digest)| match digest {
            FileDigest::Present(mut signature) => {
                if inputs.config.source_date_epoch.is_some() {
                    signature.mtime_ns = 0;
                    signature.ctime_ns = None;
                }
                Some((path.to_string_lossy().into_owned(), signature))
            }
            FileDigest::Absent | FileDigest::Unreadable => None,
        })
        .collect();
    (lockfile.digest().map(str::to_string), files)
}

/// Record in `validation` each project the write vouches for, proven against
/// `lockfile`, and each other one `carried` still proves. Returns how many it
/// recorded.
fn record_projects(
    validation: &mut Validation,
    inputs: &RecordValidation<'_>,
    carried: Option<&Validation>,
    lockfile: Option<&str>,
) -> usize {
    let records = inputs.project_manifests
        .iter()
        .filter_map(|(root_dir, manifest)| {
            let importer_id = importer_id_from_root_dir(inputs.workspace_root, root_dir);
            let vouched = match inputs.vouched {
                Vouched::All => true,
                Vouched::Projects { dirs, .. } => dirs.contains(root_dir),
            };
            if vouched {
                return Some((importer_id, manifest_digest(manifest), lockfile));
            }
            let record = carried?.project(&importer_id)?;
            Some((importer_id.clone(), record.manifest.to_string(), record.lockfile))
        })
        .collect::<Vec<_>>();
    let recorded = records.len();
    for (importer_id, manifest, lockfile) in records {
        validation.insert_project(importer_id, manifest, lockfile);
    }
    recorded
}

/// The input files every project shares besides the lockfile: the patch
/// files and the pnpmfiles.
pub(crate) fn shared_input_paths(
    workspace_root: &Path,
    config: &Config,
    pnpmfiles: &[String],
) -> Vec<PathBuf> {
    let patches = config.patched_dependencies.iter().flat_map(|patches| patches.values());
    patches
        .map(|path| {
            let path = Path::new(path);
            if path.is_absolute() { path.to_path_buf() } else { workspace_root.join(path) }
        })
        .chain(pnpmfiles.iter().map(PathBuf::from))
        .collect()
}

/// Whether the projects `previous` vouched for were proven against the
/// workspace-wide inputs `state` and `files` record now. A filtered install
/// that changed any of them changed what an unselected project's tree should
/// be too.
fn shared_inputs_match(
    previous: &WorkspaceState,
    state: &WorkspaceState,
    shared_inputs: &[PathBuf],
    files: &BTreeMap<String, FileSignature>,
) -> bool {
    let Some(validation) = previous.validation.as_ref() else {
        return false;
    };
    let digest_in = |files: &BTreeMap<String, FileSignature>, path: &Path| {
        files
            .get(&*path.to_string_lossy())
            .map(|signature| signature.digest.clone())
    };
    // Only a resolving install records `autoDedupe`, and it shapes no tree.
    // Empty maps and `enableGlobalVirtualStore: false` match their omitted defaults.
    let settings = |state: &WorkspaceState| {
        let mut settings = state.settings.clone();
        settings.auto_dedupe = None;
        if settings.allow_builds.as_ref().is_some_and(BTreeMap::is_empty) {
            settings.allow_builds = None;
        }
        if settings.enable_global_virtual_store == Some(false) {
            settings.enable_global_virtual_store = None;
        }
        if settings.package_extensions
            .as_ref()
            .and_then(serde_json::Value::as_object)
            .is_some_and(serde_json::Map::is_empty)
        {
            settings.package_extensions = None;
        }
        settings
    };
    settings(previous) == settings(state)
        && previous.pnpmfiles == state.pnpmfiles
        && previous.config_dependencies == state.config_dependencies
        && shared_inputs
            .iter()
            .all(|path| digest_in(&validation.files, path) == digest_in(files, path))
}
