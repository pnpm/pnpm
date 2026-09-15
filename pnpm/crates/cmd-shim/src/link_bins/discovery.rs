use super::{
    Arc, FsReadDir, FsReadFile, LinkBinsError, PackageBinSource, Path, Value, io,
    parse_manifest_bytes,
};

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

/// Read the installed packages directly under `modules_dir`, including
/// scoped packages one directory deeper.
/// Add the packages under one `@scope/` directory.
///
/// Only `NotFound` (and a `@`-prefixed file, which is not a scope directory)
/// is plausibly skippable — a concurrent scope-dir delete. Other errors,
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

fn read_package<Sys: FsReadFile>(
    location: &Path,
) -> Result<Option<PackageBinSource>, LinkBinsError> {
    let manifest_path = location.join("package.json");
    let bytes = match Sys::read_file(&manifest_path) {
        Ok(bytes) => bytes,
        // A missing manifest and a non-directory entry both mean the
        // same thing: this is not a package. Users do drop stray files
        // into `node_modules`, and one of them must not fail the
        // install.
        Err(error)
            if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::NotADirectory) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(LinkBinsError::ReadManifest { path: manifest_path, error }),
    };
    let manifest: Value = parse_manifest_bytes(&bytes)
        .map_err(|error| LinkBinsError::ParseManifest { path: manifest_path, error })?;
    Ok(Some(PackageBinSource::new(location.to_path_buf(), Arc::new(manifest))))
}
