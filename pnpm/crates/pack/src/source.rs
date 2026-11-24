use std::{
    collections::HashSet,
    fs::File,
    io::Read,
    path::{
        Component,
        Path,
        PathBuf,
    },
};

use indexmap::IndexMap;
use pnpm_exportable_manifest::{
    PrivateWorkspaceDependencies,
    override_publish_config,
    read_readme_file,
};
use pnpm_package_manifest::safe_read_project_manifest_from_dir;
use pnpm_package_name::is_valid_old_npm_package_name;
use pnpm_reporter::Reporter;
use serde_json::Value;

use super::{
    PackError,
    PackOptions,
    contents::executable_sources,
    output::{
        normalize_tarball_name,
        strip_build_metadata,
    },
    package_file_sources,
    prevent_bundled_dependencies_with_pnp,
};

/// The manifests a pack starts from: the project's, the publish directory's
/// (after the prepack scripts) and the exportable one the tarball carries.
pub(super) struct PackSource {
    pub(super) entry_manifest: Value,
    pub(super) dir: PathBuf,
    pub(super) publish_manifest: Value,
    pub(super) normalized_name: String,
    pub(super) published_version: String,
    pub(super) bins: Vec<PathBuf>,
    pub(super) private_workspace_dependencies: PrivateWorkspaceDependencies,
}

pub(super) async fn prepare_source<Reporter>(opts: &PackOptions) -> Result<PackSource, PackError>
where
    Reporter: self::Reporter,
{
    let entry_manifest = read_manifest(&opts.dir)?;
    prevent_bundled_dependencies_with_pnp(opts.manifest.node_linker, &entry_manifest)?;
    let dir = resolve_publish_directory(&opts.dir, &entry_manifest)?;

    if !opts.scripts.ignore {
        opts.scripts.run_if_present::<Reporter>(
            &opts.dir,
            &["prepack", "prepare"],
            &entry_manifest,
        )?;
    }

    // Re-read the manifest from `dir`: a `prepack` / `prepare` script
    // may have rewritten it.
    let manifest = read_manifest(&dir)?;
    prevent_bundled_dependencies_with_pnp(opts.manifest.node_linker, &manifest)?;

    validate_source_identity(&manifest)?;

    let (mut publish_manifest, private_workspace_dependencies) =
        opts.manifest.export::<Reporter>(&opts.dir, &dir, &manifest).await?;

    let (normalized_name, published_version) = published_identity(&mut publish_manifest)?;
    let bins = executable_sources(&publish_manifest, &manifest, &dir);
    Ok(PackSource {
        entry_manifest,
        dir,
        publish_manifest,
        normalized_name,
        published_version,
        bins,
        private_workspace_dependencies,
    })
}

/// Disk sources for a package whose file selection uses no pnpmfile hooks.
/// Returns `None` when a pack lifecycle script can generate or change those sources.
pub fn static_pack_sources(
    project_dir: &Path,
    workspace_dir: Option<&Path>,
    entry_manifest: &Value,
) -> Result<Option<impl Iterator<Item = PathBuf>>, PackError> {
    if ["prepack", "prepare"]
        .iter()
        .any(|stage| entry_manifest["scripts"][stage].is_string())
    {
        return Ok(None);
    }
    let source_dir = resolve_publish_directory(project_dir, entry_manifest)?;
    let mut manifest = if source_dir == project_dir {
        entry_manifest.clone()
    } else {
        read_manifest(&source_dir)?
    };
    if let Some(object) = manifest.as_object_mut() {
        override_publish_config(object);
    }
    Ok(Some(
        package_file_sources(project_dir, workspace_dir, &source_dir, &manifest)?.into_values(),
    ))
}

/// Check the source manifest before applying publish overrides and hooks.
fn validate_source_identity(manifest: &Value) -> Result<(), PackError> {
    package_name(manifest)?;
    let version = manifest
        .get("version")
        .and_then(Value::as_str)
        .filter(|version| !version.is_empty())
        .ok_or(PackError::PackageVersionNotFound)?;
    if is_invalid_tarball_version(version) {
        return Err(PackError::InvalidPackageVersion { version: version.to_string() });
    }
    Ok(())
}

fn package_name(manifest: &Value) -> Result<&str, PackError> {
    let name = manifest.get("name").ok_or(PackError::PackageNameNotFound)?;
    let name = name
        .as_str()
        .ok_or_else(|| PackError::InvalidPackageName { name: name.to_string() })?;
    if name.is_empty() {
        return Err(PackError::PackageNameNotFound);
    }
    if !is_valid_old_npm_package_name(name) {
        return Err(PackError::InvalidPackageName { name: name.to_string() });
    }
    Ok(name)
}

/// The tarball name and version the publish manifest settles on.
///
/// Semver build metadata (the `+<build>` segment) is stripped so the tarball
/// name, the packed manifest and any registry metadata all agree on the
/// version. See [pnpm/pnpm#11518](https://github.com/pnpm/pnpm/issues/11518).
///
/// The name and version are read back off the publish manifest so a
/// `publishConfig.name` rename or a `beforePacking` hook modification reaches
/// the filename too. Neither went through [`validate_source_identity`], so both are
/// validated here: they land in the tarball path, where a separator or `.` /
/// `..` segment would smuggle path components into the join and write outside
/// `dest_dir`.
fn published_identity(publish_manifest: &mut Value) -> Result<(String, String), PackError> {
    let raw_version = publish_manifest
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("");
    let published_version = strip_build_metadata(raw_version).to_string();
    if is_invalid_tarball_version(&published_version) {
        return Err(PackError::InvalidPackageVersion { version: raw_version.to_string() });
    }
    if let Some(object) = publish_manifest.as_object_mut() {
        object.insert("version".to_string(), Value::String(published_version.clone()));
    }
    let published_name = package_name(publish_manifest)?;
    Ok((normalize_tarball_name(published_name), published_version))
}

fn is_invalid_tarball_version(version: &str) -> bool {
    version.is_empty()
        || version == "."
        || version == ".."
        || version.contains('/')
        || version.contains('\\')
        || version.contains(':')
}

/// The readme is always reported as part of the published manifest, matching the npm CLI, so a
/// registry can render it on the package page. `embed_readme` only controls whether the readme is
/// additionally written into the `package.json` inside the tarball (via
/// [`pnpm_exportable_manifest::create_exportable_manifest`]), which is why it is filled in on the returned manifest here
/// rather than in the packed one.
pub(super) fn with_registry_readme(mut manifest: Value, dir: &Path) -> Result<Value, PackError> {
    if manifest
        .get("readme")
        .is_some_and(|readme| !readme.is_null())
    {
        return Ok(manifest);
    }
    let readme = read_readme_file(dir)
        .map_err(|source| PackError::ReadFile { path: dir.display().to_string(), source })?;
    if let Some(readme) = readme
        && let Some(object) = manifest.as_object_mut()
    {
        object.insert("readme".to_string(), Value::String(readme));
    }
    Ok(manifest)
}

/// Read the raw manifest under `dir`, erroring when it is absent.
fn read_manifest(dir: &Path) -> Result<Value, PackError> {
    match safe_read_project_manifest_from_dir(dir) {
        Ok(Some(manifest)) => Ok(manifest),
        Ok(None) => Err(PackError::ManifestNotFound { dir: dir.display().to_string() }),
        Err(source) => Err(PackError::ReadManifest(source)),
    }
}

/// Resolve `publishConfig.directory` to a relative path inside `project_dir`.
///
/// Validation stays lexical so build outputs symlinked into the package keep
/// packing (see [pnpm/pnpm#17390](https://github.com/pnpm/pnpm/issues/17390)).
/// Root, prefix, and `..` segments are rejected so paths cannot escape
/// `project_dir` or a symlink target.
fn resolve_publish_directory(project_dir: &Path, manifest: &Value) -> Result<PathBuf, PackError> {
    let Some(raw_value) = manifest
        .get("publishConfig")
        .and_then(|config| config.get("directory"))
        .filter(|value| !value.is_null())
    else {
        return Ok(project_dir.to_path_buf());
    };
    let Some(relative) = raw_value.as_str() else {
        return Err(PackError::InvalidPublishDirectory { dir: raw_value.to_string() });
    };
    if relative.is_empty() {
        return Ok(project_dir.to_path_buf());
    }
    if is_invalid_publish_directory(relative) {
        return Err(PackError::InvalidPublishDirectory { dir: relative.to_string() });
    }
    let mut dir = project_dir.to_path_buf();
    for component in Path::new(relative).components() {
        if let Component::Normal(segment) = component {
            dir.push(segment);
        }
    }
    Ok(dir)
}

fn is_invalid_publish_directory(raw: &str) -> bool {
    let path = Path::new(raw);
    if raw.contains(['\0', ':']) || path.is_absolute() || raw.starts_with(['/', '\\']) {
        return true;
    }
    path.components()
        .any(|component| {
            matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_))
        })
        || raw
            .split(['/', '\\'])
            .any(is_dot_traversal_segment)
}

fn is_dot_traversal_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment
            .bytes()
            .all(|byte| byte == b'.' || byte == b' ')
}

/// Reject a packed executable whose shebang line ends with CRLF, which Unix
/// shells cannot run. Only the bins that land in the tarball are read, the
/// same set [`crate::tarball::build_tarball`] marks executable.
pub(super) fn check_packed_bins_for_crlf(
    files_map: &IndexMap<String, PathBuf>,
    bins: &[PathBuf],
) -> Result<(), PackError> {
    let bin_set: HashSet<&Path> = bins
        .iter()
        .map(PathBuf::as_path)
        .collect();
    for (name, source) in files_map {
        if !bin_set.contains(source.as_path()) {
            continue;
        }
        let mut head = Vec::with_capacity(SHEBANG_SCAN_LEN as usize);
        File::open(source)
            .map(|file| file.take(SHEBANG_SCAN_LEN))
            .and_then(|mut take| take.read_to_end(&mut head))
            .map_err(|error| PackError::ReadFile {
                path: source.display().to_string(),
                source: error,
            })?;
        if has_shebang_with_crlf(&head) {
            let path = name
                .strip_prefix("package/")
                .unwrap_or(name)
                .to_string();
            return Err(PackError::BinCrlf { path });
        }
    }
    Ok(())
}

const SHEBANG_SCAN_LEN: u64 = 4096;

fn has_shebang_with_crlf(bytes: &[u8]) -> bool {
    let mut bytes = bytes;
    if bytes.starts_with(b"\xEF\xBB\xBF") {
        bytes = &bytes[3..];
    }
    if !bytes.starts_with(b"#!") {
        return false;
    }
    for &byte in &bytes[2..] {
        if byte == b'\r' {
            return true;
        }
        if byte == b'\n' {
            return false;
        }
    }
    false
}
