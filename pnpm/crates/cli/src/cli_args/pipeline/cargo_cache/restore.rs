use std::{
    fs::{
        self,
        File,
        FileTimes,
    },
    io,
    path::Path,
};

pub(super) fn clone_file(source: &Path, target: &Path) -> io::Result<()> {
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

/// Content changes can retain mtimes, and relocated build scripts can retain
/// the publisher's paths. Invalidate all freshness records for new inputs, or
/// local units and build-script runs when moving an exact-input snapshot.
pub(super) fn invalidate_fingerprints(
    root: &Path,
    local_packages: Option<&[String]>,
) -> io::Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if entry.file_name() != ".fingerprint" {
            invalidate_fingerprints(&entry.path(), local_packages)?;
            continue;
        }
        let Some(packages) = local_packages else {
            fs::remove_dir_all(entry.path())?;
            continue;
        };
        invalidate_local_fingerprints(&entry.path(), packages)?;
    }
    Ok(())
}

/// Drop the fingerprints of the workspace's own packages and of every
/// build script. The rest describe dependencies the restored snapshot
/// still matches.
fn invalidate_local_fingerprints(fingerprint_dir: &Path, packages: &[String]) -> io::Result<()> {
    for fingerprint in fs::read_dir(fingerprint_dir)? {
        let fingerprint = fingerprint?;
        if !fingerprint.file_type()?.is_dir() {
            continue;
        }
        let name = fingerprint
            .file_name()
            .to_string_lossy()
            .into_owned();
        let local = packages
            .iter()
            .any(|package| name.starts_with(&format!("{package}-")));
        let build_script = fs::read_dir(fingerprint.path())?
            .collect::<io::Result<Vec<_>>>()?
            .iter()
            .any(|file| {
                file.file_name()
                    .to_string_lossy()
                    .starts_with("run-build-script")
            });
        if local || build_script {
            fs::remove_dir_all(fingerprint.path())?;
        }
    }
    Ok(())
}
