//! The `pack` command.
//!
//! [`api`] packs a single project into a `.tgz`: it runs the
//! `prepack` / `prepare` lifecycle scripts, builds the publish manifest
//! (via [`pacquet_exportable_manifest`]), computes the file list (via
//! [`pacquet_fs_packlist`]), writes the reproducible gzipped tarball,
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

mod capabilities;
mod manifest_entry;
mod tarball;

#[cfg(test)]
mod tests;

use derive_more::{Display, Error};
use manifest_entry::is_manifest_entry;
use miette::Diagnostic;
use pacquet_catalogs_types::Catalogs;
use pacquet_cmd_shim::get_bins_from_package_manifest;
use pacquet_config::NodeLinker;
use pacquet_executor::{
    LifecycleScriptError, RunPostinstallHooks, ScriptsPrependNodePath, run_lifecycle_hook,
};
use pacquet_exportable_manifest::{
    CreateExportableManifestError, CreateExportableManifestOptions, create_exportable_manifest,
};
use pacquet_fs_packlist::{PacklistError, packlist};
use pacquet_package_manifest::{PackageManifestError, safe_read_package_json_from_dir};
use pacquet_reporter::Reporter;
use pacquet_resolving_parse_wanted_dependency::is_valid_old_npm_package_name;
use serde_json::Value;
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
};

pub use capabilities::{FsAtomicWrite, FsCreateDirAll, FsFileLen, FsReadFile, Host};

/// The single supported manifest basename. pacquet only reads
/// `package.json`; the name appears in the "name/version not defined"
/// errors, matching pnpm's `manifestFileName`.
const MANIFEST_FILE_NAME: &str = "package.json";

/// Inputs for [`api`]. The CLI maps the resolved [`pacquet_config::Config`]
/// and command-line flags onto this struct.
pub struct PackOptions {
    /// Project directory to pack.
    pub dir: PathBuf,
    /// Parsed workspace catalogs, for `catalog:` specifier rewriting.
    pub catalogs: Catalogs,
    /// Skip the `prepack` / `prepare` / `postpack` lifecycle scripts.
    pub ignore_scripts: bool,
    /// `--unsafe-perm`: run lifecycle scripts without dropping privileges.
    /// Threaded from [`pacquet_config::Config::unsafe_perm`] so packing
    /// honors the same policy (and `TMPDIR` isolation) as an install.
    pub unsafe_perm: bool,
    /// Embed the project's `README.md` into the published manifest.
    pub embed_readme: bool,
    /// gzip compression level (`0..=9`); `None` uses the zlib default.
    pub pack_gzip_level: Option<u32>,
    /// Node linker mode; `bundledDependencies` only work under
    /// [`NodeLinker::Hoisted`].
    pub node_linker: NodeLinker,
    /// Keep `packageManager` and publish-lifecycle scripts in the packed
    /// manifest.
    pub skip_manifest_obfuscation: bool,
    /// `npm_config_user_agent` stamped on lifecycle scripts.
    pub user_agent: String,
    /// Extra directories prepended to `PATH` for lifecycle scripts.
    pub extra_bin_paths: Vec<PathBuf>,
    /// Extra environment variables for lifecycle scripts.
    pub extra_env: HashMap<String, String>,
    /// Workspace root, used to inject a root `LICENSE` into a
    /// sub-package tarball that lacks one.
    pub workspace_dir: Option<PathBuf>,
    /// Do everything except writing the tarball to disk.
    pub dry_run: bool,
    /// Directory to write the tarball into.
    pub pack_destination: Option<String>,
    /// Custom output path template (`%s` = name, `%v` = version).
    pub out: Option<String>,
}

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

    #[display("Failed to read {path}: {source}")]
    #[diagnostic(code(pacquet_pack::read_file))]
    ReadFile {
        path: String,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to create directory {path}: {source}")]
    #[diagnostic(code(pacquet_pack::create_dir))]
    CreateDir {
        path: String,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to write tarball to {path}: {source}")]
    #[diagnostic(code(pacquet_pack::write_tarball))]
    WriteTarball {
        path: String,
        #[error(source)]
        source: io::Error,
    },
}

/// Pack the project at `opts.dir` into a tarball and return the result.
///
/// `R` threads the reporter through the lifecycle-script emits; `Sys`
/// is the filesystem seam for the tarball write phase
/// ([`capabilities::Host`] in production).
pub fn api<Reporter, Sys>(opts: &PackOptions) -> Result<PackResult, PackError>
where
    Reporter: self::Reporter,
    Sys: FsReadFile + FsFileLen + FsCreateDirAll + FsAtomicWrite,
{
    let entry_manifest = read_manifest(&opts.dir)?;
    prevent_bundled_dependencies_without_hoisted(opts.node_linker, &entry_manifest)?;

    if !opts.ignore_scripts {
        run_scripts_if_present::<Reporter>(opts, &["prepack", "prepare"], &entry_manifest)?;
    }

    // The publish directory may differ from the project root when
    // `publishConfig.directory` redirects packing at a build output.
    let dir = match publish_config_directory(&entry_manifest) {
        Some(relative) => opts.dir.join(relative),
        None => opts.dir.clone(),
    };

    // Re-read the manifest from `dir`: a `prepack` / `prepare` script
    // may have rewritten it.
    let manifest = read_manifest(&dir)?;
    prevent_bundled_dependencies_without_hoisted(opts.node_linker, &manifest)?;

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
    // The version is interpolated into the default tarball filename
    // (`<name>-<version>.tgz`), and the manifest is attacker-controlled.
    // A separator would let `version` smuggle path components into the
    // join and write the tarball outside `dest_dir`. A real semver
    // version never contains one, so reject it.
    if version.contains('/') || version.contains('\\') {
        return Err(PackError::InvalidPackageVersion { version: version.to_string() });
    }

    let modules_dir = opts.dir.join("node_modules");
    let mut publish_manifest = create_exportable_manifest(
        &dir,
        &manifest,
        &CreateExportableManifestOptions {
            catalogs: &opts.catalogs,
            modules_dir: Some(&modules_dir),
            skip_manifest_obfuscation: opts.skip_manifest_obfuscation,
            embed_readme: opts.embed_readme,
        },
    )
    .map_err(PackError::CreateManifest)?;

    // Strip semver build metadata (the `+<build>` segment) so the
    // tarball name, the packed manifest, and any registry metadata all
    // agree on the version. See pnpm/pnpm#11518.
    let published_version =
        strip_build_metadata(publish_manifest.get("version").and_then(Value::as_str).unwrap_or(""))
            .to_string();
    if let Some(object) = publish_manifest.as_object_mut() {
        object.insert("version".to_string(), Value::String(published_version.clone()));
    }

    let normalized_name = normalize_tarball_name(name);
    let (tarball_name, pack_destination) =
        resolve_output(opts, &normalized_name, &published_version)?;

    let files = packlist(&dir, &publish_manifest).map_err(PackError::Packlist)?;
    let mut files_map = build_files_map(&dir, &files);
    inject_workspace_license(opts, &dir, &files, &mut files_map);

    let manifest_json = serde_json::to_string_pretty(&publish_manifest)
        .expect("publish manifest serializes to JSON")
        .into_bytes();

    let dest_dir = resolve_dest_dir(&dir, pack_destination.as_deref());
    if !opts.dry_run {
        Sys::create_dir_all(&dest_dir).map_err(|source| PackError::CreateDir {
            path: dest_dir.display().to_string(),
            source,
        })?;
    }

    // The size pass must run before `postpack`, which may delete
    // prepack-generated files that were packed. See pnpm/pnpm#12775.
    let unpacked_size = unpacked_size::<Sys>(&files_map, manifest_json.len() as u64)?;
    let contents = packed_contents(&files_map);

    if !opts.dry_run {
        let bins = executable_sources(&publish_manifest, &manifest, &dir);
        let dest_file = dest_dir.join(&tarball_name);
        Sys::atomic_write(&dest_file, &mut |writer| {
            tarball::build_tarball::<Sys>(
                writer,
                &files_map,
                &manifest_json,
                &bins,
                opts.pack_gzip_level,
            )
        })
        .map_err(|source| PackError::WriteTarball {
            path: dest_file.display().to_string(),
            source,
        })?;
        if !opts.ignore_scripts {
            run_scripts_if_present::<Reporter>(opts, &["postpack"], &entry_manifest)?;
        }
    }

    let tarball_path = packed_tarball_path(&opts.dir, &dir, &dest_dir, &tarball_name);

    Ok(PackResult { published_manifest: publish_manifest, contents, tarball_path, unpacked_size })
}

/// Project a [`PackResult`] into its JSON shape.
#[must_use]
pub fn to_pack_result_json(result: &PackResult) -> PackResultJson {
    let manifest = &result.published_manifest;
    PackResultJson {
        name: manifest.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
        version: manifest.get("version").and_then(Value::as_str).unwrap_or_default().to_string(),
        filename: result.tarball_path.clone(),
        files: result.contents.iter().map(|path| PackFile { path: path.clone() }).collect(),
    }
}

/// Render packed results the way `pnpm pack` prints them: pretty JSON
/// under `--json`, otherwise a per-package "Tarball Contents / Details"
/// block.
#[must_use]
pub fn format_pack_output(results: &[PackResultJson], json: bool, unicode: bool) -> String {
    if json {
        return if results.len() > 1 {
            serde_json::to_string_pretty(&results)
        } else {
            serde_json::to_string_pretty(&results[0])
        }
        .expect("pack result serializes to JSON");
    }

    let prefix = if unicode { "📦 " } else { "package:" };
    results
        .iter()
        .map(|result| {
            // `name` / `version` / `filename` and the file paths are
            // manifest- and filesystem-derived, so strip control
            // characters before they reach the terminal — a file named
            // with raw ANSI escapes would otherwise spoof the output.
            let files = result
                .files
                .iter()
                .map(|file| sanitize_for_terminal(&file.path))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "{prefix} {name}@{version}\nTarball Contents\n{files}\nTarball Details\n{filename}",
                name = sanitize_for_terminal(&result.name),
                version = sanitize_for_terminal(&result.version),
                filename = sanitize_for_terminal(&result.filename),
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Strip control characters (keeping `\n` / `\t`) from text headed for the
/// terminal, so a manifest- or filesystem-derived value can't emit raw
/// escape sequences. JSON output is left untouched — it is data, not a
/// terminal rendering.
fn sanitize_for_terminal(text: &str) -> std::borrow::Cow<'_, str> {
    if text
        .chars()
        .any(|character| character.is_control() && character != '\n' && character != '\t')
    {
        std::borrow::Cow::Owned(
            text.chars()
                .filter(|character| {
                    !character.is_control() || *character == '\n' || *character == '\t'
                })
                .collect(),
        )
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

/// Read the raw manifest under `dir`, erroring when it is absent.
fn read_manifest(dir: &Path) -> Result<Value, PackError> {
    match safe_read_package_json_from_dir(dir) {
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

/// Whether a JSON value is truthy under JavaScript's coercion rules, so
/// the guard fires for exactly the values pnpm's `if (bundledDependencies)`
/// check rejects — `false`, `0`, `""`, and `null`/absent are skipped,
/// while a non-empty array, object, number, string, or `true` all fire.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(boolean) => *boolean,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(string) => !string.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn node_linker_str(node_linker: NodeLinker) -> &'static str {
    match node_linker {
        NodeLinker::Isolated => "isolated",
        NodeLinker::Hoisted => "hoisted",
        NodeLinker::Pnp => "pnp",
    }
}

/// Run the named lifecycle scripts that the manifest actually declares,
/// in order. Mirrors upstream's `runScriptsIfPresent`; the Rust port is
/// a plain loop rather than upstream's bound partial application.
fn run_scripts_if_present<Reporter: self::Reporter>(
    opts: &PackOptions,
    script_names: &[&str],
    manifest: &Value,
) -> Result<(), PackError> {
    let scripts = manifest.get("scripts");
    if !script_names.iter().any(|name| script_body(scripts, name).is_some()) {
        return Ok(());
    }

    let dep_path = opts.dir.to_string_lossy().into_owned();
    let root_modules_dir = realpath_missing(&opts.dir.join("node_modules"));
    let run_opts = RunPostinstallHooks {
        dep_path: &dep_path,
        pkg_root: &opts.dir,
        root_modules_dir: &root_modules_dir,
        init_cwd: &opts.dir,
        extra_bin_paths: &opts.extra_bin_paths,
        extra_env: &opts.extra_env,
        node_execpath: None,
        npm_execpath: None,
        node_gyp_path: None,
        user_agent: Some(&opts.user_agent),
        unsafe_perm: opts.unsafe_perm,
        node_gyp_bin: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::default(),
        script_shell: None,
        optional: false,
    };
    let parent_env: HashMap<String, String> = std::env::vars().collect();

    for &script_name in script_names {
        let Some(script) = script_body(scripts, script_name) else { continue };
        run_lifecycle_hook::<Reporter>(script_name, script, &run_opts, manifest, &parent_env)
            .map_err(PackError::Lifecycle)?;
    }
    Ok(())
}

/// The body of `scripts.<name>` when it is a non-empty string.
fn script_body<'a>(scripts: Option<&'a Value>, name: &str) -> Option<&'a str> {
    scripts?.get(name).and_then(Value::as_str).filter(|script| !script.is_empty())
}

/// `name.replace('@', '').replace('/', '-')`, first-occurrence only, to
/// match the JS `String.prototype.replace(string, ...)` semantics that
/// build a tarball's default filename.
fn normalize_tarball_name(name: &str) -> String {
    name.replacen('@', "", 1).replacen('/', "-", 1)
}

/// Resolve `(tarball_name, pack_destination)` from the `--out` template
/// or the default `<name>-<version>.tgz`. `--out` and
/// `--pack-destination` are mutually exclusive.
fn resolve_output(
    opts: &PackOptions,
    normalized_name: &str,
    version: &str,
) -> Result<(String, Option<String>), PackError> {
    let Some(out) = &opts.out else {
        return Ok((format!("{normalized_name}-{version}.tgz"), opts.pack_destination.clone()));
    };
    if opts.pack_destination.is_some() {
        return Err(PackError::OutAndPackDestination);
    }
    let prepared = out.replace("%s", normalized_name).replace("%v", version);
    let prepared_path = Path::new(&prepared);
    // `--out .`, `--out ..`, or `--out ""` resolve to no filename; the
    // join would then target a directory and the write would fail with a
    // confusing OS error, so reject the option up front.
    let Some(tarball_name) =
        prepared_path.file_name().map(|name| name.to_string_lossy().into_owned())
    else {
        return Err(PackError::InvalidOut { out: out.clone() });
    };
    let parent =
        prepared_path.parent().map(|dir| dir.to_string_lossy().into_owned()).unwrap_or_default();
    let pack_destination =
        if parent.is_empty() { opts.pack_destination.clone() } else { Some(parent) };
    Ok((tarball_name, pack_destination))
}

/// Map each packed path to `package/<path>` → absolute source, in
/// packlist order.
fn build_files_map(dir: &Path, files: &[String]) -> indexmap::IndexMap<String, PathBuf> {
    files.iter().map(|file| (format!("package/{file}"), dir.join(file))).collect()
}

/// Resolve the directory the tarball is written into.
fn resolve_dest_dir(dir: &Path, pack_destination: Option<&str>) -> PathBuf {
    match pack_destination {
        Some(destination) if Path::new(destination).is_absolute() => PathBuf::from(destination),
        Some(destination) => dir.join(destination),
        None => dir.to_path_buf(),
    }
}

/// The reported tarball path: relative to the project root when the
/// tarball landed there, otherwise the absolute destination path.
fn packed_tarball_path(
    project_dir: &Path,
    publish_dir: &Path,
    dest_dir: &Path,
    tarball_name: &str,
) -> String {
    if project_dir != dest_dir {
        return dest_dir.join(tarball_name).display().to_string();
    }
    pathdiff::diff_paths(publish_dir.join(tarball_name), project_dir)
        .unwrap_or_else(|| PathBuf::from(tarball_name))
        .display()
        .to_string()
}

/// Absolute source paths that should be marked executable in the
/// tarball: the publish manifest's resolved bins plus any
/// `publishConfig.executableFiles`.
fn executable_sources(publish_manifest: &Value, manifest: &Value, dir: &Path) -> Vec<PathBuf> {
    let mut bins: Vec<PathBuf> =
        get_bins_from_package_manifest::<pacquet_cmd_shim::Host>(publish_manifest, dir)
            .into_iter()
            .map(|command| command.path)
            .collect();
    if let Some(executable_files) = manifest
        .get("publishConfig")
        .and_then(|config| config.get("executableFiles"))
        .and_then(Value::as_array)
    {
        for file in executable_files.iter().filter_map(Value::as_str) {
            bins.push(dir.join(file));
        }
    }
    bins
}

/// Append a workspace-root `LICENSE` to a sub-package tarball that lacks
/// one.
fn inject_workspace_license(
    opts: &PackOptions,
    dir: &Path,
    files: &[String],
    files_map: &mut indexmap::IndexMap<String, PathBuf>,
) {
    let Some(workspace_dir) = &opts.workspace_dir else { return };
    if dir == workspace_dir || files.iter().any(|file| contains_license(file)) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(workspace_dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_license_filename(&name) {
            continue;
        }
        // Only inject a regular file. A directory named `LICENSE` would
        // fail the later read/size pass with "Is a directory", and a
        // symlink could point outside the workspace and leak its target's
        // bytes into the published tarball. `DirEntry::file_type` does not
        // follow symlinks, so `is_file()` rejects both — matching the
        // symlink-skipping `read_readme_file` does in `exportable-manifest`.
        if entry.file_type().is_ok_and(|file_type| file_type.is_file()) {
            files_map.insert(format!("package/{name}"), workspace_dir.join(&name));
        }
    }
}

/// Total uncompressed size of every tar entry. Manifest entries use the
/// serialized publish manifest's length rather than the on-disk file's,
/// since `pack` rewrites them.
fn unpacked_size<Sys: FsFileLen>(
    files_map: &indexmap::IndexMap<String, PathBuf>,
    manifest_json_len: u64,
) -> Result<u64, PackError> {
    let mut total = 0u64;
    for (name, source) in files_map {
        total += if is_manifest_entry(name) {
            manifest_json_len
        } else {
            Sys::file_len(source).map_err(|source_err| PackError::ReadFile {
                path: source.display().to_string(),
                source: source_err,
            })?
        };
    }
    Ok(total)
}

/// De-duplicated, locale-sorted list of the tarball's contents.
/// Manifest entries collapse to `package.json`; the `package/` prefix is
/// stripped from the rest.
fn packed_contents(files_map: &indexmap::IndexMap<String, PathBuf>) -> Vec<String> {
    let mut seen = HashSet::new();
    let contents: Vec<String> = files_map
        .keys()
        .map(|name| {
            if is_manifest_entry(name) {
                "package.json".to_string()
            } else {
                name.strip_prefix("package/").unwrap_or(name).to_string()
            }
        })
        .filter(|item| seen.insert(item.clone()))
        .collect();
    // Decorate each path with its lowercase form once, rather than
    // recomputing `to_lowercase` for both sides on every comparison.
    let mut decorated: Vec<(String, String)> =
        contents.into_iter().map(|item| (item.to_lowercase(), item)).collect();
    decorated.sort_by(|(left_lower, left), (right_lower, right)| {
        left_lower.cmp(right_lower).then_with(|| case_precedence_tiebreak(left, right))
    });
    decorated.into_iter().map(|(_, item)| item).collect()
}

/// Tie-breaker for [`packed_contents`]' `localeCompare(b, 'en')`
/// approximation: once two ASCII path strings compare equal
/// case-insensitively, give a lowercase character precedence over its
/// uppercase counterpart. Full ICU collation is not a workspace
/// dependency; this reproduces `en` ordering for plain file paths, where
/// the two agree.
fn case_precedence_tiebreak(left: &str, right: &str) -> Ordering {
    for (left_char, right_char) in left.chars().zip(right.chars()) {
        if left_char == right_char {
            continue;
        }
        return match (left_char.is_lowercase(), right_char.is_lowercase()) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => left_char.cmp(&right_char),
        };
    }
    left.len().cmp(&right.len())
}

/// Whether a packed path looks like a license file, matching upstream's
/// unanchored `/LICEN[CS]E(?:\..+)?/i` presence test.
fn contains_license(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("license") || lower.contains("licence")
}

/// Whether a root filename matches the `LICEN{S,C}E{,.*}` glob pnpm
/// uses to find a workspace-root license to inject.
fn is_license_filename(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(lower.as_str(), "license" | "licence")
        || lower.starts_with("license.")
        || lower.starts_with("licence.")
}

/// `version` without its `+<build>` metadata segment.
fn strip_build_metadata(version: &str) -> &str {
    version.split_once('+').map_or(version, |(base, _)| base)
}

/// Resolve a path's realpath, falling back to the input when it doesn't
/// exist yet. Mirrors upstream's `realpathMissing` for the lifecycle
/// `INIT_CWD`-adjacent modules dir.
fn realpath_missing(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
