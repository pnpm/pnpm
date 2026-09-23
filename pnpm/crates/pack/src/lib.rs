//! The `pack` command.
//!
//! [`api`] packs a single project into a `.tgz`: it runs the
//! `prepack` / `prepare` lifecycle scripts, builds the publish manifest
//! (via [`pnpm_exportable_manifest`]), computes the file list (via
//! [`pnpm_fs_packlist`]), writes the reproducible gzipped tarball,
//! then runs `postpack`. [`to_pack_result_json`] and
//! [`format_pack_output`] render the result the way the CLI prints it.
//!
//! The recursive (`-r`) orchestration — selecting and topologically
//! sorting the workspace projects — lives in the CLI command alongside
//! pacquet's other recursive commands, and calls [`api`] per project.
//!
//! The filesystem write phase is injected through the [`Host`] /
//! capability seam so its `PermissionDenied` / `ENOSPC` branches are
//! testable; everything else runs on real `std::fs` and is covered by
//! `tempfile` fixtures.

pub use capabilities::{FsAtomicWrite, FsCreateDirAll, FsFileLen, FsReadFile, Host};
pub use contents::sort_paths_en_locale;
pub use options::{
    PackManifestOptions, PackOptions, PackOutputLocks, PackOutputOptions, PackScripts,
};
pub use output::{format_pack_output, pack_output_path, to_pack_result_json};

mod capabilities;
mod collation;
mod manifest_entry;
mod options;
mod tarball;

#[cfg(test)]
mod tests;

use derive_more::{Display, Error};
use manifest_entry::is_manifest_entry;
use miette::Diagnostic;
use pnpm_catalogs_types::Catalogs;
use pnpm_cmd_shim::get_bins_from_package_manifest;
use pnpm_config::NodeLinker;
use pnpm_executor::{
    LifecycleScriptError, RunPostinstallHooks, ScriptsPrependNodePath, run_lifecycle_hook,
};
use pnpm_exportable_manifest::{
    CreateExportableManifestError, CreateExportableManifestOptions, create_exportable_manifest,
    read_readme_file,
};
use pnpm_fs::lexical_normalize;
use pnpm_fs_packlist::{PacklistError, PacklistOptions, packlist_with_options};
use pnpm_hooks::{HookContext, LogFn, PnpmfileHooks};
use pnpm_package_manifest::{
    ManifestFormat, PackageManifestError, is_truthy, project_manifest_path,
    safe_read_project_manifest_from_dir,
};
use pnpm_package_name::is_valid_old_npm_package_name;
use pnpm_reporter::{HookLog, LogEvent, LogLevel, Reporter};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

/// The published manifest basename used in identity validation errors.
const MANIFEST_FILE_NAME: &str = "package.json";

/// Result of packing one project.
#[derive(Debug)]
pub struct PackResult {
    /// The manifest packed inside the tarball.
    pub published_manifest: Value,
    /// Sorted, de-duplicated list of the tarball's contents (paths
    /// relative to the package root, `package.json` for the manifest).
    pub contents: Vec<String>,
    /// Path to the written tarball, relative to `dir` when it landed
    /// there, otherwise the absolute destination path.
    pub tarball_path: String,
    /// Total uncompressed size of all files in the tarball, in bytes.
    pub unpacked_size: u64,
}

/// JSON-serializable projection of a [`PackResult`].
#[derive(serde::Serialize)]
pub struct PackResultJson {
    pub name: String,
    pub version: String,
    pub filename: String,
    pub files: Vec<PackFile>,
}

/// One entry of [`PackResultJson::files`].
#[derive(serde::Serialize)]
pub struct PackFile {
    pub path: String,
}

/// Failures from [`api`]. Codes that pnpm defines are preserved
/// byte-for-byte so `pnpm.io/errors` references and log consumers keep
/// matching.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PackError {
    #[display("No package.json found in {dir}")]
    #[diagnostic(code(ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND))]
    ManifestNotFound { dir: String },

    #[diagnostic(transparent)]
    ReadManifest(#[error(source)] PackageManifestError),

    #[display("{field} does not work with \"nodeLinker: {node_linker}\"")]
    #[diagnostic(
        code(ERR_PNPM_BUNDLED_DEPENDENCIES_WITHOUT_HOISTED),
        help(
            "Add \"nodeLinker: hoisted\" to pnpm-workspace.yaml or delete {field} from the root package.json to resolve this error"
        )
    )]
    BundledDependenciesWithoutHoisted { field: &'static str, node_linker: &'static str },

    #[display("Package name is not defined in the {MANIFEST_FILE_NAME}.")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_NAME_NOT_FOUND))]
    PackageNameNotFound,

    #[display("Invalid package name \"{name}\".")]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_NAME))]
    InvalidPackageName { name: String },

    #[display("Package version is not defined in the {MANIFEST_FILE_NAME}.")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_VERSION_NOT_FOUND))]
    PackageVersionNotFound,

    #[display("Invalid package version \"{version}\".")]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_VERSION))]
    InvalidPackageVersion { version: String },

    #[display("Cannot use --pack-destination and --out together")]
    #[diagnostic(code(ERR_PNPM_INVALID_OPTION))]
    OutAndPackDestination,

    #[display("Invalid --out value \"{out}\": it does not resolve to a file name")]
    #[diagnostic(code(ERR_PNPM_INVALID_OPTION))]
    InvalidOut { out: String },

    #[diagnostic(transparent)]
    CreateManifest(#[error(source)] CreateExportableManifestError),

    #[diagnostic(transparent)]
    Packlist(#[error(source)] PacklistError),

    #[diagnostic(transparent)]
    Lifecycle(#[error(source)] LifecycleScriptError),

    #[display("The \"beforePacking\" hook from {pnpmfile} failed: {message}")]
    #[diagnostic(code(ERR_PNPM_PACK_BEFORE_PACKING))]
    BeforePacking { pnpmfile: String, message: String },

    #[display("Failed to read {path}: {source}")]
    #[diagnostic(code(ERR_PNPM_PACK_READ_FILE))]
    ReadFile {
        path: String,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to create directory {path}: {source}")]
    #[diagnostic(code(ERR_PNPM_PACK_CREATE_DIR))]
    CreateDir {
        path: String,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to write tarball to {path}: {source}")]
    #[diagnostic(code(ERR_PNPM_PACK_WRITE_TARBALL))]
    WriteTarball {
        path: String,
        #[error(source)]
        source: io::Error,
    },
}

pub async fn api<Reporter, Sys>(opts: &PackOptions) -> Result<PackResult, PackError>
where
    Reporter: self::Reporter,
    Sys: FsReadFile + FsFileLen + FsCreateDirAll + FsAtomicWrite,
{
    let source = prepare_source::<Reporter>(opts).await?;
    let (tarball_name, pack_destination) =
        resolve_output(&opts.output, &source.normalized_name, &source.published_version)?;
    let files_map = packed_files_map(opts, &source)?;
    let manifest_json = serde_json::to_string_pretty(&source.publish_manifest)
        .expect("publish manifest serializes to JSON")
        .into_bytes();

    let dest_dir = resolve_dest_dir(&source.dir, pack_destination.as_deref());
    if !opts.output.dry_run {
        create_dest_dir::<Sys>(&dest_dir)?;
    }

    // The size pass must run before `postpack`, which may delete
    // prepack-generated files that were packed. See pnpm/pnpm#12775.
    let unpacked_size = unpacked_size::<Sys>(&files_map, manifest_json.len() as u64)?
        + opts.output.injected_files
            .iter()
            .map(|(_, bytes)| bytes.len() as u64)
            .sum::<u64>();
    let contents = packed_contents_with_injected(&files_map, &opts.output.injected_files);

    if !opts.output.dry_run {
        let packed = PackedTarball {
            dest_file: dest_dir.join(&tarball_name),
            files_map: &files_map,
            manifest_json: &manifest_json,
        };
        write_tarball::<Sys>(&opts.output, &source, &packed).await?;
        if !opts.scripts.ignore {
            opts.scripts.run_if_present::<Reporter>(
                &opts.dir,
                &["postpack"],
                &source.entry_manifest,
            )?;
        }
    }

    let tarball_path = packed_tarball_path(&opts.dir, &source.dir, &dest_dir, &tarball_name);
    let published_manifest = with_registry_readme(source.publish_manifest, &source.dir)?;
    Ok(PackResult { published_manifest, contents, tarball_path, unpacked_size })
}

/// The manifests a pack starts from: the project's, the publish directory's
/// (after the prepack scripts) and the exportable one the tarball carries.
struct PackSource {
    entry_manifest: Value,
    dir: PathBuf,
    manifest: Value,
    publish_manifest: Value,
    normalized_name: String,
    published_version: String,
}

async fn prepare_source<Reporter: self::Reporter>(
    opts: &PackOptions,
) -> Result<PackSource, PackError> {
    let entry_manifest = read_manifest(&opts.dir, opts.manifest.preferred_format)?;
    prevent_bundled_dependencies_without_hoisted(opts.manifest.node_linker, &entry_manifest)?;

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
    let manifest = read_manifest(&dir, opts.manifest.preferred_format)?;
    prevent_bundled_dependencies_without_hoisted(opts.manifest.node_linker, &manifest)?;

    let name = packed_identity(&manifest)?;

    let mut publish_manifest = opts.manifest.export::<Reporter>(&opts.dir, &dir, &manifest).await?;

    let (normalized_name, published_version) = published_identity(&mut publish_manifest, name)?;
    Ok(PackSource {
        entry_manifest,
        dir,
        manifest,
        publish_manifest,
        normalized_name,
        published_version,
    })
}

fn packed_files_map(
    opts: &PackOptions,
    source: &PackSource,
) -> Result<indexmap::IndexMap<String, PathBuf>, PackError> {
    let files = packlist_with_options(
        &source.dir,
        &source.publish_manifest,
        PacklistOptions { workspace_dir: opts.workspace_dir.as_deref() },
    )
    .map_err(PackError::Packlist)?;
    let mut files_map = build_files_map(&source.dir, &files);
    files_map.retain(|name, _| !is_manifest_entry(name));
    files_map.insert(
        "package/package.json".to_string(),
        project_manifest_path(&source.dir, opts.manifest.preferred_format),
    );
    inject_workspace_license(opts, &source.dir, &mut files_map);
    // A composed entry supersedes any same-named on-disk file (e.g. a stale
    // committed CHANGELOG.md), so drop it from the file map before packing.
    for (name, _) in &opts.output.injected_files {
        files_map.shift_remove(name);
    }
    Ok(files_map)
}

fn create_dest_dir<Sys: FsCreateDirAll>(dest_dir: &Path) -> Result<(), PackError> {
    Sys::create_dir_all(dest_dir)
        .map_err(|source| PackError::CreateDir { path: dest_dir.display().to_string(), source })
}

struct PackedTarball<'a> {
    dest_file: PathBuf,
    files_map: &'a indexmap::IndexMap<String, PathBuf>,
    manifest_json: &'a [u8],
}

async fn write_tarball<Sys: FsReadFile + FsAtomicWrite>(
    opts: &PackOutputOptions,
    source: &PackSource,
    packed: &PackedTarball<'_>,
) -> Result<(), PackError> {
    let bins = executable_sources(&source.publish_manifest, &source.manifest, &source.dir);
    let _output_guard = match &opts.locks {
        Some(locks) => Some(locks.lock(&packed.dest_file).await),
        None => None,
    };
    Sys::atomic_write(&packed.dest_file, &mut |writer| {
        tarball::build_tarball::<Sys>(
            writer,
            packed.files_map,
            packed.manifest_json,
            &bins,
            opts.gzip_level,
            &opts.injected_files,
        )
    })
    .map_err(|error| PackError::WriteTarball {
        path: packed.dest_file.display().to_string(),
        source: error,
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

/// Pack the project at `opts.dir` into a tarball and return the result.
///
/// `R` threads the reporter through the lifecycle-script emits; `Sys`
/// is the filesystem seam for the tarball write phase
/// ([`capabilities::Host`] in production).
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
/// [`create_exportable_manifest`]), which is why it is filled in on the returned manifest here
/// rather than in the packed one.
fn with_registry_readme(mut manifest: Value, dir: &Path) -> Result<Value, PackError> {
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
fn read_manifest(dir: &Path, preferred: Option<ManifestFormat>) -> Result<Value, PackError> {
    match safe_read_project_manifest_from_dir(dir, preferred) {
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

/// Reject `bundledDependencies` / `bundleDependencies` unless the node
/// linker is `hoisted` — the only mode that materializes the bundled
/// trees a publish would carry.
fn prevent_bundled_dependencies_without_hoisted(
    node_linker: NodeLinker,
    manifest: &Value,
) -> Result<(), PackError> {
    if node_linker == NodeLinker::Hoisted {
        return Ok(());
    }
    for field in ["bundledDependencies", "bundleDependencies"] {
        if manifest.get(field).is_some_and(is_truthy) {
            return Err(PackError::BundledDependenciesWithoutHoisted {
                field,
                node_linker: node_linker_str(node_linker),
            });
        }
    }
    Ok(())
}

fn node_linker_str(node_linker: NodeLinker) -> &'static str {
    match node_linker {
        NodeLinker::Isolated => "isolated",
        NodeLinker::Hoisted => "hoisted",
        NodeLinker::Pnp => "pnp",
    }
}

mod output;

use output::{
    normalize_tarball_name, packed_tarball_path, realpath_missing, resolve_dest_dir,
    resolve_output, strip_build_metadata,
};

mod contents;
use contents::{
    build_files_map, executable_sources, inject_workspace_license, packed_contents_with_injected,
    unpacked_size,
};

mod lifecycle;
use lifecycle::apply_before_packing;

impl PackManifestOptions {
    async fn export<Reporter: self::Reporter>(
        &self,
        project_dir: &Path,
        dir: &Path,
        manifest: &Value,
    ) -> Result<Value, PackError> {
        let modules_dir = project_dir.join("node_modules");
        let mut publish_manifest = create_exportable_manifest(
            dir,
            manifest,
            &CreateExportableManifestOptions {
                catalogs: &self.catalogs,
                workspace_dir: self.catalogs_dir.as_deref(),
                modules_dir: Some(&modules_dir),
                skip_manifest_obfuscation: self.skip_obfuscation,
                embed_readme: self.embed_readme,
            },
        )
        .map_err(PackError::CreateManifest)?;

        // Run `beforePacking` hooks against the built manifest, before the
        // file list is computed, so a hook that rewrites `files` / `bin` /
        // dependency fields is honored. pnpm runs it inside
        // `createExportableManifest`; pacquet applies it here because
        // `create_exportable_manifest` is a pure, synchronous transform.
        publish_manifest = apply_before_packing::<Reporter>(
            project_dir,
            dir,
            publish_manifest,
            &self.before_packing_hooks,
        )
        .await?;

        Ok(publish_manifest)
    }
}
