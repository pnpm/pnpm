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

pub use capabilities::{
    FsAtomicWrite, FsCreateDirAll, FsFileLen, FsIsExecutable, FsReadFile, Host,
};
pub use contents::sort_paths_en_locale;
pub use options::{
    PackManifestOptions, PackOptions, PackOutputLocks, PackOutputOptions, PackScripts,
};
pub use output::{format_pack_output, pack_output_path, to_pack_result_json};
pub use pnpm_exportable_manifest::WorkspacePackageManifest;

mod capabilities;
mod collation;
mod manifest_entry;
mod options;
mod source;
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
};
use pnpm_fs::lexical_normalize;
use pnpm_fs_packlist::{PacklistError, PacklistOptions, packlist_with_sources};
use pnpm_hooks::{HookContext, LogFn, PnpmfileHooks};
use pnpm_package_manifest::{ManifestFormat, PackageManifestError, project_manifest_path};
use pnpm_reporter::{HookLog, LogEvent, LogLevel, Reporter};
use serde_json::Value;
use source::{PackSource, check_packed_bins_for_crlf, prepare_source, with_registry_readme};
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
            "Set \"nodeLinker: isolated\" or \"nodeLinker: hoisted\" in pnpm-workspace.yaml or delete {field} from the root package.json to resolve this error"
        )
    )]
    BundledDependenciesWithPnp { field: &'static str, node_linker: &'static str },

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

    #[display("The bin file \"{path}\" has a shebang line ending with CRLF (\\r\\n).")]
    #[diagnostic(
        code(ERR_PNPM_BIN_CRLF),
        help(
            "CRLF line endings on the shebang line break execution on Unix systems (/usr/bin/env: 'node\\r': No such file or directory). Convert line endings of \"{path}\" to LF (\\n)."
        )
    )]
    BinCrlf { path: String },
}

pub async fn api<Reporter, Sys>(opts: &PackOptions) -> Result<PackResult, PackError>
where
    Reporter: self::Reporter,
    Sys: FsReadFile + FsFileLen + FsCreateDirAll + FsAtomicWrite + FsIsExecutable,
{
    let source = prepare_source::<Reporter>(opts).await?;
    let (tarball_name, pack_destination) =
        resolve_output(&opts.output, &source.normalized_name, &source.published_version)?;
    let files_map = packed_files_map(opts, &source)?;
    warn_about_unlisted_dotenv_files::<Reporter>(&source.dir, &source.publish_manifest, &files_map);
    check_packed_bins_for_crlf(&files_map, &source.bins)?;
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
        write_tarball::<Sys>(&opts.output, &packed, &source.bins).await?;
        if !opts.scripts.ignore {
            opts.scripts.run_if_present::<Reporter>(
                &opts.dir,
                &["postpack"],
                &source.entry_manifest,
                opts.manifest.format,
            )?;
        }
    }

    let tarball_path = packed_tarball_path(&opts.dir, &source.dir, &dest_dir, &tarball_name);
    let published_manifest = with_registry_readme(source.publish_manifest, &source.dir)?;
    Ok(PackResult { published_manifest, contents, tarball_path, unpacked_size })
}

fn packed_files_map(
    opts: &PackOptions,
    source: &PackSource,
) -> Result<indexmap::IndexMap<String, PathBuf>, PackError> {
    let files = packlist_with_sources(
        &source.dir,
        &source.publish_manifest,
        PacklistOptions {
            workspace_dir: opts.workspace_dir.as_deref(),
            bundled_dependencies_dir: Some(&opts.dir),
        },
    )
    .map_err(PackError::Packlist)?;
    let mut files_map = build_files_map(files);
    files_map.retain(|name, _| !is_manifest_entry(name));
    files_map.insert(
        "package/package.json".to_string(),
        project_manifest_path(&source.dir, opts.manifest.format),
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

async fn write_tarball<Sys: FsReadFile + FsIsExecutable + FsAtomicWrite>(
    opts: &PackOutputOptions,
    packed: &PackedTarball<'_>,
    bins: &[PathBuf],
) -> Result<(), PackError> {
    let _output_guard = match &opts.locks {
        Some(locks) => Some(locks.lock(&packed.dest_file).await),
        None => None,
    };
    Sys::atomic_write(&packed.dest_file, &mut |writer| {
        tarball::build_tarball::<Sys>(
            writer,
            packed.files_map,
            packed.manifest_json,
            bins,
            opts.gzip_level,
            &opts.injected_files,
        )
    })
    .map_err(|error| PackError::WriteTarball {
        path: packed.dest_file.display().to_string(),
        source: error,
    })
}

mod output;

use output::{packed_tarball_path, realpath_missing, resolve_dest_dir, resolve_output};

mod contents;
use contents::{
    build_files_map, inject_workspace_license, packed_contents_with_injected, unpacked_size,
};

mod dotenv;
use dotenv::warn_about_unlisted_dotenv_files;

mod lifecycle;
use lifecycle::apply_before_packing;

mod node_linker;
use node_linker::prevent_bundled_dependencies_with_pnp;

impl PackManifestOptions {
    pub(crate) async fn export<Reporter: self::Reporter>(
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
                workspace_packages: self.workspace_packages.as_deref(),
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
