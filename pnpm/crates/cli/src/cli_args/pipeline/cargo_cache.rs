pub(super) use snapshot_key::snapshot_entry;
mod snapshot_key;
use super::paths::{check_ancestors, validate_relative_path};
use fingerprints::invalidate_fingerprints;
use pnpm_crypto_hash::{create_hex_hash, create_hex_hash_from_file};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    fs::{File, FileTimes, OpenOptions},
    io,
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(test)]
mod tests;

const INPUT_RECORD: &str = ".pnpm-cargo-inputs-v1";

pub(super) struct CargoCache {
    pub target: PathBuf,
    _lock: File,
}

#[derive(Serialize, Deserialize)]
struct Snapshot {
    key: String,
    files: Vec<SnapshotFile>,
    local_packages: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct SnapshotFile {
    path: PathBuf,
    hash: String,
}

impl CargoCache {
    pub fn open(project: &Path, directory: &str) -> io::Result<Self> {
        let relative = Path::new(directory);
        validate_relative_path(relative)?;
        let ignored = Command::new("git")
            .args(["check-ignore", "--quiet", "--"])
            .arg(format!("{directory}/"))
            .current_dir(project)
            .status()?;
        if !ignored.success() {
            return Err(io::Error::other(format!(
                "Cargo target directory must be ignored by Git: {directory}",
            )));
        }
        let target = project.join(relative);
        check_ancestors(project, relative)?;
        let parent = target.parent().expect("relative target has a parent");
        fs::create_dir_all(parent)?;
        let common = command_output(
            "git",
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            project,
            &BTreeMap::new(),
        )?;
        let locks = Path::new(common.trim()).join("pnpm-cargo-locks");
        fs::create_dir_all(&locks)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(locks.join(create_hex_hash(&target.to_string_lossy())))?;
        lock.lock()?;
        check_ancestors(project, relative)?;
        Ok(Self {
            target,
            _lock: lock,
        })
    }

    /// Restores only an absent target. Every task still runs, including a hit.
    pub fn restore(&self, entry: &Path, key: &str) -> io::Result<bool> {
        self.check_snapshot_location(entry)?;
        if self.target.try_exists()? {
            return Ok(false);
        }
        let snapshot: Snapshot = serde_json::from_slice(&fs::read(entry.join("manifest.json"))?)?;
        if snapshot.key != key {
            return Err(io::Error::other("Cargo snapshot input identity differs"));
        }
        let staging = tempfile::Builder::new()
            .prefix(".pnpm-cargo-restore-")
            .tempdir_in(self.target.parent().expect("target parent"))?;
        for file in snapshot.files {
            validate_relative_path(&file.path)?;
            check_ancestors(entry, &Path::new("files").join(&file.path))?;
            let source = entry.join("files").join(&file.path);
            let target = staging.path().join(&file.path);
            clone_file(&source, &target)?;
            if create_hex_hash_from_file(&target)? != file.hash {
                return Err(io::Error::other(format!(
                    "Cargo snapshot file failed integrity: {}",
                    file.path.display(),
                )));
            }
        }
        invalidate_fingerprints(staging.path(), Some(&snapshot.local_packages))?;
        fs::write(staging.path().join(INPUT_RECORD), key)?;
        // Cooperating pipeline writers hold the target lock outside the cache.
        // Cache eviction can remove a source, but never this private staging tree.
        fs::rename(staging.path(), &self.target)?;
        Ok(true)
    }

    pub fn prepare(&self, key: &str) -> io::Result<()> {
        if self.target.try_exists()?
            && fs::read_to_string(self.target.join(INPUT_RECORD)).ok().as_deref() != Some(key)
        {
            invalidate_fingerprints(&self.target, None)?;
        }
        Ok(())
    }

    pub fn publish(&self, entry: &Path, key: &str, local_packages: &[String]) -> io::Result<()> {
        self.check_snapshot_location(entry)?;
        fs::write(self.target.join(INPUT_RECORD), key)?;
        if entry.try_exists()? {
            return Ok(());
        }
        let parent = entry.parent().expect("snapshot entry has a parent");
        fs::create_dir_all(parent)?;
        let staging = tempfile::Builder::new().prefix(".publish-").tempdir_in(parent)?;
        let mut files = self.clone_target_into(staging.path())?;
        files.sort_by(|left, right| left.path.cmp(&right.path));
        fs::write(
            staging.path().join("manifest.json"),
            serde_json::to_vec(&Snapshot {
                key: key.to_string(),
                files,
                local_packages: local_packages.to_vec(),
            })?,
        )?;
        match fs::rename(staging.path(), entry) {
            Ok(()) => Ok(()),
            Err(_) if entry.is_dir() => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Copy every file of the build directory into the staging area,
    /// returning what the snapshot manifest records for them.
    fn clone_target_into(&self, staging: &Path) -> io::Result<Vec<SnapshotFile>> {
        let mut files = Vec::new();
        let mut pending = vec![PathBuf::new()];
        while let Some(relative) = pending.pop() {
            for item in fs::read_dir(self.target.join(&relative))? {
                Self::clone_entry(&item?, &relative, staging, &mut pending, &mut files)?;
            }
        }
        Ok(files)
    }

    /// Clone one build-directory entry into the staging area, queueing a
    /// directory for its own walk.
    fn clone_entry(
        item: &fs::DirEntry,
        parent: &Path,
        staging: &Path,
        pending: &mut Vec<PathBuf>,
        files: &mut Vec<SnapshotFile>,
    ) -> io::Result<()> {
        let relative = parent.join(item.file_name());
        if relative == Path::new(INPUT_RECORD) {
            return Ok(());
        }
        let kind = item.file_type()?;
        if kind.is_dir() {
            pending.push(relative);
            return Ok(());
        }
        if !kind.is_file() {
            return Err(io::Error::other(format!(
                "Cargo snapshot contains a non-regular file: {}",
                item.path().display(),
            )));
        }
        let destination = staging.join("files").join(&relative);
        clone_file(&item.path(), &destination)?;
        files.push(SnapshotFile {
            path: relative,
            hash: create_hex_hash_from_file(&destination)?,
        });
        Ok(())
    }

    fn check_snapshot_location(&self, entry: &Path) -> io::Result<()> {
        let target = pnpm_fs::realpath_missing(&self.target)?;
        let entry = pnpm_fs::realpath_missing(entry)?;
        if entry.starts_with(&target) || target.starts_with(&entry) {
            return Err(io::Error::other(format!(
                "Cargo snapshot {} overlaps the build directory {}",
                entry.display(),
                target.display(),
            )));
        }
        Ok(())
    }
}

pub(super) fn cache_environment(
    extra: &std::collections::HashMap<String, String>,
    declared: &[String],
) -> BTreeMap<String, String> {
    env::vars()
        .chain(
            extra
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        )
        .filter(|(key, _)| {
            key.starts_with("CARGO_")
                || key.starts_with("RUST")
                || key.starts_with("CC")
                || key.starts_with("CXX")
                || key.starts_with("AR")
                || key.starts_with("CFLAGS")
                || key.starts_with("CPPFLAGS")
                || key.starts_with("LDFLAGS")
                || key.starts_with("PKG_CONFIG")
                || key == "PATH"
                || key == "SDKROOT"
                || key == "MACOSX_DEPLOYMENT_TARGET"
                || declared.contains(key)
        })
        .filter(|(key, _)| key != "CARGO_TARGET_DIR" && key != "CARGO_BUILD_BUILD_DIR")
        .collect()
}

fn command_output(
    program: &str,
    args: &[&str],
    project: &Path,
    environment: &BTreeMap<String, String>,
) -> io::Result<String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(project)
        .envs(environment)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "{program} {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr),
        )));
    }
    String::from_utf8(output.stdout).map_err(io::Error::other)
}

fn clone_file(source: &Path, target: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if !metadata.is_file() {
        return Err(io::Error::other(format!(
            "Cargo cache file is not regular: {}",
            source.display(),
        )));
    }
    fs::create_dir_all(target.parent().expect("file parent"))?;
    reflink_copy::reflink_or_copy(source, target)?;
    let mut options = File::options();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.access_mode(windows_sys::Win32::Storage::FileSystem::FILE_WRITE_ATTRIBUTES);
    }
    options
        .open(target)?
        .set_times(FileTimes::new().set_modified(metadata.modified()?))?;
    fs::set_permissions(target, metadata.permissions())
}

mod fingerprints;
