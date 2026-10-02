//! A record that the wanted lockfile on disk is what an `autoDedupe` resolution
//! wrote. A lockfile does not say whether it was deduplicated, so `autoDedupe`
//! keeps installs off the up-to-date path unless this record vouches for the
//! lockfile. A deduplicating resolution converges, so after a `--lockfile-only`
//! one writes the lockfile, the install records digests of what it read. A
//! later `--lockfile-only` install whose digests match takes the up-to-date
//! path.
//! Like the repeat-install fast path, it does not re-resolve for a setting the
//! workspace state leaves out, a change inside a `link:` target that is not a
//! workspace project, or versions published since. A local file that is missing
//! or not a regular file leaves nothing to record. Records live in
//! `<cache_dir>/auto-dedupe-baselines/`, one per lockfile path.

use pnpm_config::PNPM_VERSION;
use pnpm_crypto_hash::{create_hex_hash, create_hex_hash_bytes, create_hex_hash_from_file};
use pnpm_lockfile::{Lockfile, LockfileResolution};
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace_state::WorkspaceStateSettings;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const RECORD_DIR: &str = "auto-dedupe-baselines";

/// The inputs of one install's deduplicating resolution besides the
/// lockfile, and where the record for its lockfile lives.
pub(crate) struct AutoDedupeBaseline {
    record: PathBuf,
    lockfile: PathBuf,
    inputs: String,
}

/// What a deduplicating resolution read besides the lockfile.
pub(crate) struct BaselineInputs<'a> {
    pub(crate) settings: &'a WorkspaceStateSettings,
    /// `ignorePnpmfile` also skips the lockfile's `pnpmfileChecksum`
    /// comparison, so nothing else would notice it flip.
    pub(crate) ignore_pnpmfile: bool,
    pub(crate) workspace_root: &'a Path,
    pub(crate) project_manifests: &'a [(PathBuf, &'a PackageManifest)],
}

impl AutoDedupeBaseline {
    pub(crate) fn new(cache_dir: &Path, lockfile: PathBuf, inputs: &BaselineInputs<'_>) -> Self {
        let record = cache_dir
            .join(RECORD_DIR)
            .join(create_hex_hash_bytes(lockfile.as_os_str().as_encoded_bytes()));
        AutoDedupeBaseline { record, lockfile, inputs: inputs_digest(inputs) }
    }

    /// Whether a deduplicating resolution under these inputs wrote the
    /// lockfile as it is on disk now, `lockfile` being its parsed document.
    /// Compares the cheapest digest first, so a changed manifest or lockfile
    /// misses without reading the local packages.
    pub(crate) fn matches(&self, lockfile: &Lockfile) -> bool {
        let Ok(recorded) = fs::read_to_string(&self.record) else { return false };
        let Some((inputs, rest)) = recorded.split_once('\n') else { return false };
        let Some((file, local)) = rest.split_once('\n') else { return false };
        inputs == self.inputs
            && self
                .lockfile_file_digest()
                .is_some_and(|digest| digest == file)
            && self
                .local_packages_digest(lockfile)
                .is_some_and(|digest| digest == local)
    }

    /// Record the lockfile on disk, `lockfile` being the document written
    /// there, as the output of a deduplicating resolution under these inputs.
    /// A failure only costs a later install its shortcut, so it is logged and
    /// otherwise ignored.
    pub(crate) fn record(&self, lockfile: &Lockfile) {
        let Some(digest) = self.digest(lockfile) else { return };
        let written = self.record
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| pnpm_fs::write_atomic(&self.record, digest.as_bytes()));
        if let Err(error) = written {
            tracing::debug!(
                target: "pacquet::install",
                ?error,
                record = %self.record.display(),
                "failed to record the autoDedupe baseline",
            );
        }
    }

    fn digest(&self, lockfile: &Lockfile) -> Option<String> {
        let file = self.lockfile_file_digest()?;
        let local = self.local_packages_digest(lockfile)?;
        Some(format!("{}\n{file}\n{local}", self.inputs))
    }

    fn lockfile_file_digest(&self) -> Option<String> {
        create_hex_hash_from_file(&self.lockfile).ok()
    }

    fn local_packages_digest(&self, lockfile: &Lockfile) -> Option<String> {
        local_packages_digest(self.lockfile.parent()?, lockfile)
    }
}

/// The manifest of every local directory and the bytes of every local
/// tarball `lockfile` resolves, relative to the lockfile's directory.
/// `None` when one is not a regular file: a directory without a
/// `package.json` resolves to whichever ancestor publishes from it, and
/// opening a FIFO or a device could block.
fn local_packages_digest(lockfile_dir: &Path, lockfile: &Lockfile) -> Option<String> {
    let mut files: Vec<PathBuf> = lockfile.packages
        .iter()
        .flatten()
        .filter_map(|(_, package)| local_package_file(lockfile_dir, &package.resolution))
        .collect();
    files.sort_unstable();
    files.dedup();
    let digests = files
        .iter()
        .map(|file| {
            if !file.is_file() {
                return None;
            }
            let path = create_hex_hash_bytes(file.as_os_str().as_encoded_bytes());
            let content = create_hex_hash_from_file(file).ok()?;
            Some(format!("{path}\n{content}"))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(create_hex_hash(&digests.join("\n")))
}

fn local_package_file(lockfile_dir: &Path, resolution: &LockfileResolution) -> Option<PathBuf> {
    match resolution {
        LockfileResolution::Directory(directory) => {
            Some(lockfile_dir.join(&directory.directory).join("package.json"))
        }
        LockfileResolution::Tarball(tarball) => tarball.tarball
            .strip_prefix("file:")
            .map(|path| lockfile_dir.join(path)),
        _ => None,
    }
}

/// The pnpm version, the workspace-state settings, `ignorePnpmfile` and
/// every project manifest by importer id, as one JSON document.
fn inputs_digest(inputs: &BaselineInputs<'_>) -> String {
    let manifests: BTreeMap<String, &serde_json::Value> = inputs
        .project_manifests
        .iter()
        .map(|(project_dir, manifest)| {
            let importer_id =
                pnpm_workspace::importer_id_from_root_dir(inputs.workspace_root, project_dir);
            (importer_id, manifest.value())
        })
        .collect();
    let inputs =
        serde_json::to_vec(&(PNPM_VERSION, inputs.settings, inputs.ignore_pnpmfile, manifests))
            .expect("the record's inputs serialize to JSON");
    create_hex_hash_bytes(&inputs)
}

#[cfg(test)]
mod tests;
