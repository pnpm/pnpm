use std::{
    ffi::OsStr,
    fs::{self, File, FileTimes},
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
    #[cfg(not(target_os = "wasi"))]
    {
        pnpm_fs::CopyPermissions::of_metadata(&metadata).apply_to_path(target)
    }
    #[cfg(target_os = "wasi")]
    {
        let file = pnpm_fs::open_file_without_following(target)?;
        pnpm_fs::copy_permissions(source)?.apply_to_file(&file)
    }
}

/// Content changes can retain mtimes, and relocated build scripts can retain
/// the publisher's paths. Invalidate all freshness records for new inputs, or
/// local units and build-script runs when moving an exact-input snapshot.
///
/// Recognizes both Cargo build-dir layouts: `.fingerprint/<package>-<hash>`
/// and `build/<package>/<hash>/fingerprint`.
pub(super) fn invalidate_fingerprints(
    root: &Path,
    local_packages: Option<&[String]>,
) -> io::Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            invalidate_directory(&entry.path(), local_packages)?;
        }
    }
    Ok(())
}

fn invalidate_directory(path: &Path, local_packages: Option<&[String]>) -> io::Result<()> {
    if path.file_name() == Some(OsStr::new(".fingerprint")) {
        return match local_packages {
            None => fs::remove_dir_all(path),
            Some(packages) => invalidate_local_fingerprints(path, packages),
        };
    }
    let Some(package) = unit_fingerprint_package(path) else {
        return invalidate_fingerprints(path, local_packages);
    };
    if is_stale_unit(path, package, local_packages)? {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn is_stale_unit(
    fingerprint: &Path,
    package: &str,
    local_packages: Option<&[String]>,
) -> io::Result<bool> {
    let Some(packages) = local_packages else { return Ok(true) };
    Ok(packages
        .iter()
        .any(|local| local == package)
        || runs_build_script(fingerprint)?)
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
        if local || runs_build_script(&fingerprint.path())? {
            fs::remove_dir_all(fingerprint.path())?;
        }
    }
    Ok(())
}

/// The package of a `build/<package>/<hash>/fingerprint` directory.
fn unit_fingerprint_package(path: &Path) -> Option<&str> {
    if path.file_name()? != "fingerprint" {
        return None;
    }
    let package_dir = path.parent()?.parent()?;
    if package_dir.parent()?.file_name()? != "build" {
        return None;
    }
    package_dir.file_name()?.to_str()
}

fn runs_build_script(fingerprint: &Path) -> io::Result<bool> {
    Ok(fs::read_dir(fingerprint)?
        .collect::<io::Result<Vec<_>>>()?
        .iter()
        .any(|file| {
            file.file_name()
                .to_string_lossy()
                .starts_with("run-build-script")
        }))
}
