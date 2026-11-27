use super::{
    Arc,
    FsReadDir,
    FsReadFile,
    LinkBinsError,
    PackageBinSource,
    Path,
    Value,
    io,
};
use pnpm_package_manifest::{
    PROJECT_MANIFEST_BASENAMES,
    parse_manifest_bytes,
    parse_project_manifest,
};
use std::path::PathBuf;

/// Read the installed packages directly under `modules_dir`, including
/// scoped packages one directory deeper.
pub fn collect_packages_in_modules_dir<Sys>(
    modules_dir: &Path,
) -> Result<Vec<PackageBinSource>, LinkBinsError>
where
    Sys: FsReadDir + FsReadFile,
{
    let mut packages = Vec::new();

    let entries = match Sys::read_dir(modules_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(packages),
        Err(error) => {
            return Err(LinkBinsError::ReadModulesDir { dir: modules_dir.to_path_buf(), error });
        }
    };

    for path in entries {
        let Some(name) = path.file_name() else {
            continue;
        };
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        if name_str.starts_with('@') {
            collect_scope_packages::<Sys>(&path, &mut packages)?;
            continue;
        }
        if let Some(pkg) = read_package::<Sys>(&path)? {
            packages.push(pkg);
        }
    }

    Ok(packages)
}

/// Add the packages under one `@scope/` directory.
///
/// Only `NotFound` (and a `@`-prefixed file, which is not a scope directory)
/// is plausibly skippable, a concurrent scope-dir delete. Other errors,
/// `PermissionDenied`, `EIO` or an `AppArmor` deny, would silently drop every
/// bin under this scope, so they surface as `ReadModulesDir`, matching the
/// policy the per-`modules_dir` read uses.
fn collect_scope_packages<Sys>(
    path: &Path,
    packages: &mut Vec<PackageBinSource>,
) -> Result<(), LinkBinsError>
where
    Sys: FsReadDir + FsReadFile,
{
    let scope_entries = match Sys::read_dir(path) {
        Ok(entries) => entries,
        Err(error)
            if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::NotADirectory) =>
        {
            return Ok(());
        }
        Err(error) => {
            return Err(LinkBinsError::ReadModulesDir { dir: path.to_path_buf(), error });
        }
    };
    for sub_path in scope_entries {
        if let Some(pkg) = read_package::<Sys>(&sub_path)? {
            packages.push(pkg);
        }
    }
    Ok(())
}

/// The package at `location` as a bin source. `None` when
/// [`read_package_manifest`] finds no manifest there.
pub fn read_package<Sys: FsReadFile>(
    location: &Path,
) -> Result<Option<PackageBinSource>, LinkBinsError> {
    Ok(read_package_manifest::<Sys>(location)?
        .map(|manifest| PackageBinSource::new(location.to_path_buf(), Arc::new(manifest))))
}

/// The manifest of the package at `location`, in the first format the
/// directory holds, in [`PROJECT_MANIFEST_BASENAMES`] order.
///
/// A missing manifest and a non-directory entry both mean the same thing:
/// this is not a package. Users do drop stray files into `node_modules`, and
/// one of them must not fail the install.
pub fn read_package_manifest<Sys: FsReadFile>(
    location: &Path,
) -> Result<Option<Value>, LinkBinsError> {
    for basename in PROJECT_MANIFEST_BASENAMES {
        let manifest_path = location.join(basename);
        let bytes = match Sys::read_file(&manifest_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) if error.kind() == io::ErrorKind::NotADirectory => return Ok(None),
            Err(error) => return Err(LinkBinsError::ReadManifest { path: manifest_path, error }),
        };
        return parse_package_manifest(manifest_path, &bytes).map(Some);
    }
    Ok(None)
}

/// A `package.json` may hold any JSON value, which is how
/// [`parse_manifest_bytes`] reads the manifest of a registry package.
fn parse_package_manifest(path: PathBuf, bytes: &[u8]) -> Result<Value, LinkBinsError> {
    if path
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        return parse_manifest_bytes(bytes)
            .map_err(|error| LinkBinsError::ParseManifest { path, error });
    }
    let contents = match str::from_utf8(bytes) {
        Ok(contents) => contents,
        Err(error) => {
            let error = io::Error::new(io::ErrorKind::InvalidData, error);
            return Err(LinkBinsError::ReadManifest { path, error });
        }
    };
    parse_project_manifest(&path, contents).map_err(LinkBinsError::ReadProjectManifest)
}
