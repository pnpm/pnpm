use std::{
    collections::HashSet,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use indexmap::IndexMap;
use pnpm_exportable_manifest::read_readme_file;
use pnpm_package_manifest::{ManifestFormat, safe_read_project_manifest_from_dir};
use pnpm_package_name::is_valid_old_npm_package_name;
use pnpm_reporter::Reporter;
use serde_json::Value;

use super::{
    PackError, PackOptions,
    contents::executable_sources,
    output::{normalize_tarball_name, strip_build_metadata},
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
}

pub(super) async fn prepare_source<Reporter>(opts: &PackOptions) -> Result<PackSource, PackError>
where
    Reporter: self::Reporter,
{
    let entry_manifest = read_manifest(&opts.dir, opts.manifest.format)?;
    prevent_bundled_dependencies_with_pnp(opts.manifest.node_linker, &entry_manifest)?;

    if !opts.scripts.ignore {
        opts.scripts.run_if_present::<Reporter>(
            &opts.dir,
            &["prepack", "prepare"],
            &entry_manifest,
        )?;
    }

    // The publish directory may differ from the project root when
    // `publishConfig.directory` redirects packing at a build output.
    let dir = match publish_config_directory(&entry_manifest) {
        Some(relative) => opts.dir.join(relative),
        None => opts.dir.clone(),
    };

    // Re-read the manifest from `dir`: a `prepack` / `prepare` script
    // may have rewritten it.
    let manifest = read_manifest(&dir, opts.manifest.format)?;
    prevent_bundled_dependencies_with_pnp(opts.manifest.node_linker, &manifest)?;

    let name = packed_identity(&manifest)?;

    let mut publish_manifest = opts.manifest.export::<Reporter>(&opts.dir, &dir, &manifest).await?;

    let (normalized_name, published_version) = published_identity(&mut publish_manifest, name)?;
    let bins = executable_sources(&publish_manifest, &manifest, &dir);
    Ok(PackSource {
        entry_manifest,
        dir,
        publish_manifest,
        normalized_name,
        published_version,
        bins,
    })
}

/// The name the tarball is packed under, once the manifest's name *and*
/// version are known to be publishable.
///
/// Both are interpolated into the default tarball filename
/// (`<name>-<version>.tgz`) and the manifest is attacker-controlled, so a
/// path separator in the version would let it smuggle path components into the
/// join and write the tarball outside `dest_dir`. A real semver version never
/// contains one. The version itself is read back off the publish manifest by
/// [`published_identity`], which a `publishConfig` rename can change.
fn packed_identity(manifest: &Value) -> Result<&str, PackError> {
    let name = manifest
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or(PackError::PackageNameNotFound)?;
    if !is_valid_old_npm_package_name(name) {
        return Err(PackError::InvalidPackageName { name: name.to_string() });
    }
    let version = manifest
        .get("version")
        .and_then(Value::as_str)
        .filter(|version| !version.is_empty())
        .ok_or(PackError::PackageVersionNotFound)?;
    if version.contains('/') || version.contains('\\') {
        return Err(PackError::InvalidPackageVersion { version: version.to_string() });
    }
    Ok(name)
}

/// The tarball name and version the publish manifest settles on.
///
/// Semver build metadata (the `+<build>` segment) is stripped so the tarball
/// name, the packed manifest and any registry metadata all agree on the
/// version. See [pnpm/pnpm#11518](https://github.com/pnpm/pnpm/issues/11518).
///
/// The name is read back off the publish manifest so a `publishConfig.name`
/// rename reaches the filename too. That rename never went through
/// [`packed_identity`], so it is validated here: it lands in the tarball
/// filename, where a separator would smuggle path components into the join and
/// write outside `dest_dir`.
fn published_identity(
    publish_manifest: &mut Value,
    name: &str,
) -> Result<(String, String), PackError> {
    let published_version = strip_build_metadata(
        publish_manifest
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or(""),
    )
    .to_string();
    if let Some(object) = publish_manifest.as_object_mut() {
        object.insert("version".to_string(), Value::String(published_version.clone()));
    }
    let published_name = publish_manifest
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .unwrap_or(name);
    if !is_valid_old_npm_package_name(published_name) {
        return Err(PackError::InvalidPackageName { name: published_name.to_string() });
    }
    Ok((normalize_tarball_name(published_name), published_version))
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
fn read_manifest(dir: &Path, format: ManifestFormat) -> Result<Value, PackError> {
    match safe_read_project_manifest_from_dir(dir, format) {
        Ok(Some(manifest)) => Ok(manifest),
        Ok(None) => Err(PackError::ManifestNotFound { dir: dir.display().to_string() }),
        Err(source) => Err(PackError::ReadManifest(source)),
    }
}

/// `publishConfig.directory`, when set to a non-empty string.
fn publish_config_directory(manifest: &Value) -> Option<&str> {
    manifest
        .get("publishConfig")
        .and_then(|config| config.get("directory"))
        .and_then(Value::as_str)
        .filter(|directory| !directory.is_empty())
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
            .and_then(|file| file.take(SHEBANG_SCAN_LEN).read_to_end(&mut head))
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
