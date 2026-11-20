//! Free functions backing the local-filesystem resolver. They all
//! funnel into [`resolve_spec`], which reads the package's manifest
//! (out of the archive for a tarball, off disk for a directory) once a
//! [`LocalPackageSpec`] has been chosen.

pub use filesystem_policy::LocalFilesystemPolicy;
use std::ffi::OsStr;

use std::{
    path::PathBuf,
    sync::Arc,
};

use derive_more::{
    Display,
    Error,
};
use filesystem_policy::{
    check_scheme_spec_allowed,
    find_bounded_parent_publish_manifest,
};
use miette::Diagnostic;
use pnpm_lockfile::{
    DirectoryResolution,
    IntegrityTarget,
    LockfileResolution,
};
use pnpm_package_manifest::{
    PackageManifestError,
    publishes_from_directory,
    safe_read_package_json_from_dir,
    safe_read_project_manifest_from_dir,
};
use pnpm_resolving_resolver_base::{
    LatestInfo,
    LatestQuery,
    PkgResolutionId,
    ResolveResult,
    UnsupportedProtocolError,
};
use pnpm_tarball::TarballError;
use tarball::resolve_file_spec;

use crate::parse_bare_specifier::{
    LocalPackageSpec,
    LocalSpecKind,
    ParseOptions,
    PathProtocolNotSupportedError,
    WantedLocalDependency,
    parse_local_path,
    parse_local_scheme,
};

/// Per-install knobs the dispatcher threads into every resolver call.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalResolverContext {
    /// When `true`, an absolute path in the wanted specifier is
    /// preserved in the resolved `id` rather than being relativised
    /// against the project / lockfile root. Backs pnpm's
    /// `preserveAbsolutePaths` config (off by default).
    pub preserve_absolute_paths: bool,
    /// Which hash a `file:` tarball's integrity names.
    pub integrity_target: IntegrityTarget,
    /// Which local paths the resolver may read.
    pub filesystem_policy: LocalFilesystemPolicy,
}

/// Whether `file:` tarball resolution reads the archive on disk.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LocalTarballResolution {
    /// Read the archive's integrity and bundled `package.json` during resolution.
    #[default]
    ReadMetadata,
    /// Claim the `file:` specifier without reading the archive, leaving extraction to the caller.
    DeferToFetcher,
}

/// Per-call options.
#[derive(Debug, Clone)]
pub struct LocalResolverOptions {
    pub project_dir: PathBuf,
    /// Lockfile root. Defaults to `project_dir` when `None`.
    pub lockfile_dir: Option<PathBuf>,
    /// The workspace root, which bounds the search for a `file:` directory's
    /// publishing project. Defaults to the lockfile root when `None`.
    pub workspace_dir: Option<PathBuf>,
    /// Previously resolved entry from the lockfile, threaded so the
    /// resolver can short-circuit directory resolution when the
    /// install isn't asking for an update.
    pub current_pkg: Option<LocalCurrentPkg>,
    /// `false` keeps the lockfile pin. pnpm's
    /// `update?: false | 'compatible' | 'latest'` tri-state collapses
    /// to two states here because the local resolver only branches on
    /// truthy / falsy.
    pub update: LocalResolverUpdate,
    /// `inject-workspace-packages` / `injectWorkspacePackages` config.
    /// When set, a `workspace:` directory dep resolves to `file:` the same
    /// way a per-dep `injected` flag does.
    pub inject_workspace_packages: bool,
}

/// Lockfile-pinned slice the local resolver short-circuits on for
/// directory deps when no update is requested.
#[derive(Debug, Clone)]
pub struct LocalCurrentPkg {
    pub id: PkgResolutionId,
    pub resolution: LockfileResolution,
    pub manifest: Option<Arc<serde_json::Value>>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LocalResolverUpdate {
    /// Keep the lockfile pin.
    #[default]
    Off,
    /// Re-resolve. Pnpm uses two truthy values (`'compatible'` and
    /// `'latest'`); the local resolver treats both identically.
    On,
}

/// Outcome of a successful local resolve.
#[derive(Debug, Clone)]
pub struct LocalResolveResult {
    pub id: PkgResolutionId,
    pub manifest: Option<std::sync::Arc<serde_json::Value>>,
    pub normalized_bare_specifier: Option<String>,
    pub resolution: LockfileResolution,
    /// The `resolvedVia` tag: `workspace` for a `workspace:` specifier,
    /// `local-filesystem` for every other shape. A `workspace:` specifier
    /// gets the tag the name and range form gets in
    /// `resolving-npm-resolver`, so the deps resolver treats the directory
    /// as a workspace project whichever of the two claimed it: its
    /// `catalog:` children resolve through the catalogs, and
    /// `blockExoticSubdeps` leaves its children alone.
    pub resolved_via: &'static str,
}

impl From<LocalResolveResult> for ResolveResult {
    fn from(result: LocalResolveResult) -> Self {
        ResolveResult {
            id: result.id,
            resolution: result.resolution,
            resolved_via: result.resolved_via.to_string(),
            normalized_bare_specifier: result.normalized_bare_specifier,
            alias: None,
            policy_violation: None,
            package: pnpm_resolving_resolver_base::ResolvedPackageInfo {
                // Local resolutions don't have a `name@version` shape,
                // the canonical name lives in the fetched manifest, not
                // the resolver-time signal. Leave `name_ver` empty so
                // downstream consumers fall back to reading
                // `result.manifest`.
                name_ver: None,
                latest: None,
                published_at: None,
                manifest: result.manifest,
                non_deprecated_alternative: None,
            },
        }
    }
}

/// Error returned when bare-specifier parsing itself fails (today:
/// the `path:` protocol case). Surfaces as
/// [`PathProtocolNotSupportedError`] downstream.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum LocalSpecError {
    PathProtocolNotSupported(#[error(source)] PathProtocolNotSupportedError),
}

/// Aggregate error type returned by the public entry points.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ResolveLocalError {
    /// The wanted specifier carries an unsupported scheme. Today only
    /// `path:`; reserved for future additions.
    Spec(#[error(source)] LocalSpecError),

    /// `file:` directory or tarball points at a path that doesn't
    /// exist.
    #[display("Could not install from \"{path}\" as it does not exist.")]
    #[diagnostic(code(ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND))]
    LinkedPkgDirNotFound {
        #[error(not(source))]
        path: String,
    },

    /// `<spec.fetchSpec>` exists but isn't a directory (ENOTDIR).
    #[display("Could not install from \"{path}\" as it is not a directory.")]
    #[diagnostic(code(ERR_PNPM_NOT_PACKAGE_DIRECTORY))]
    NotPackageDirectory {
        #[error(not(source))]
        path: String,
    },

    /// A specifier claimed by its path shape opens with a protocol no
    /// resolver supports, and nothing exists at that path.
    #[diagnostic(transparent)]
    UnsupportedProtocol(#[error(source)] UnsupportedProtocolError),

    /// Reading a `file:` tarball (its bytes, its sha512 integrity, or
    /// the `package.json` bundled inside it) failed.
    ReadTarball(#[error(source)] TarballError),

    /// A `file:` tarball bundles a `package.json` that names no package.
    /// The name is what gives the dep path its `<name>@` prefix, so
    /// without it the package keys no lockfile entry.
    #[display("Can't install {specifier}: Missing package name")]
    #[diagnostic(code(ERR_PNPM_MISSING_PACKAGE_NAME))]
    MissingPackageName {
        #[error(not(source))]
        specifier: String,
    },

    /// A `file:` tarball bundles a `package.json` whose name isn't a
    /// valid npm package name. The name becomes both a dep-path segment
    /// and a `node_modules/<name>` directory, so an invalid one either
    /// fails to parse back out of the dep path or writes somewhere it
    /// shouldn't.
    #[display("Refusing to install {specifier} with the invalid package name {name:?}")]
    #[diagnostic(code(ERR_PNPM_INVALID_DEPENDENCY_NAME))]
    InvalidPackageName {
        #[error(not(source))]
        specifier: String,
        name: String,
    },

    /// Reading `<spec.fetchSpec>/package.json` raised something the
    /// resolver doesn't have a specific code for (malformed JSON,
    /// permission denied, ...).
    ReadManifest(#[error(source)] PackageManifestError),

    /// A `file:` or `link:` specifier (or an escaping `workspace:` path)
    /// reached a resolver configured for [`LocalFilesystemPolicy::WorkspaceOnly`].
    #[display("Local filesystem dependency {specifier:?} cannot be resolved on the server")]
    #[diagnostic(code(ERR_PNPM_LOCAL_SPEC_NOT_ALLOWED))]
    LocalSpecNotAllowed {
        #[error(not(source))]
        specifier: String,
    },
}

/// Resolve a wanted dep declared with an explicit local scheme.
pub async fn resolve_from_local_scheme(
    ctx: &LocalResolverContext,
    wanted_dependency: &WantedLocalDependency,
    opts: &LocalResolverOptions,
) -> Result<Option<LocalResolveResult>, ResolveLocalError> {
    let project_dir = opts.project_dir.as_path();
    let lockfile_dir = opts.lockfile_dir.as_deref().unwrap_or(project_dir);
    let parse_opts = ParseOptions {
        preserve_absolute_paths: ctx.preserve_absolute_paths,
        inject_workspace_packages: opts.inject_workspace_packages,
    };
    let spec = match parse_local_scheme(wanted_dependency, project_dir, lockfile_dir, parse_opts) {
        Ok(maybe) => maybe,
        Err(err) => {
            return Err(ResolveLocalError::Spec(LocalSpecError::PathProtocolNotSupported(err)));
        }
    };
    if let Some(spec) = spec.as_ref() {
        check_scheme_spec_allowed(
            ctx.filesystem_policy,
            spec,
            &wanted_dependency.bare_specifier,
            opts,
        )?;
    }
    resolve_spec(ctx, spec, opts, None).await
}

/// Resolve a wanted dep by path shape alone, no scheme prefix.
pub async fn resolve_from_local_path(
    ctx: &LocalResolverContext,
    wanted_dependency: &WantedLocalDependency,
    opts: &LocalResolverOptions,
) -> Result<Option<LocalResolveResult>, ResolveLocalError> {
    match ctx.filesystem_policy {
        LocalFilesystemPolicy::HostFilesystem(_) => {}
        LocalFilesystemPolicy::WorkspaceOnly => return Ok(None),
    }
    let project_dir = opts.project_dir.as_path();
    let lockfile_dir = opts.lockfile_dir.as_deref().unwrap_or(project_dir);
    let parse_opts =
        ParseOptions { preserve_absolute_paths: ctx.preserve_absolute_paths, ..Default::default() };
    let spec = parse_local_path(wanted_dependency, project_dir, lockfile_dir, parse_opts);
    resolve_spec(ctx, spec, opts, Some(&wanted_dependency.bare_specifier)).await
}

/// Latest-version companion. Claims `link:` / `file:` / `workspace:`
/// specs with an empty [`LatestInfo`] so the dispatcher stops there
/// instead of routing the dep into a user-configured named-registry
/// alias of the same name.
#[must_use]
pub fn resolve_latest_from_local(query: &LatestQuery) -> Option<LatestInfo> {
    let bare = query.wanted_dependency.bare_specifier.as_deref()?;
    if bare.starts_with("link:") || bare.starts_with("file:") || bare.starts_with("workspace:") {
        return Some(LatestInfo::default());
    }
    None
}

/// `path_shaped_specifier` is the written specifier when the spec was
/// claimed by its path shape alone.
async fn resolve_spec(
    ctx: &LocalResolverContext,
    spec: Option<LocalPackageSpec>,
    opts: &LocalResolverOptions,
    path_shaped_specifier: Option<&str>,
) -> Result<Option<LocalResolveResult>, ResolveLocalError> {
    let Some(spec) = spec else {
        return Ok(None);
    };

    if matches!(spec.kind, LocalSpecKind::File) {
        return resolve_file_spec(&spec, opts, *ctx).await.map(Some);
    }

    // Directory branch.
    if let Some(current) = &opts.current_pkg
        && opts.update == LocalResolverUpdate::Off
    {
        return Ok(Some(LocalResolveResult {
            id: current.id.clone(),
            manifest: None,
            normalized_bare_specifier: Some(spec.normalized_bare_specifier),
            resolution: current.resolution.clone(),
            resolved_via: spec.resolved_via,
        }));
    }

    let manifest = match publishing_project_manifest(&spec, opts) {
        Some(manifest) => manifest,
        None => match safe_read_package_json_from_dir(&spec.fetch_spec) {
            Ok(Some(manifest)) => manifest,
            Ok(None) => synthesize_fallback_manifest(
                ctx.filesystem_policy,
                &spec,
                opts,
                path_shaped_specifier,
            )?,
            Err(err) => return Err(handle_manifest_read_failure(err, &spec)),
        },
    };

    Ok(Some(LocalResolveResult {
        id: spec.id.clone(),
        manifest: Some(std::sync::Arc::new(manifest)),
        normalized_bare_specifier: Some(spec.normalized_bare_specifier),
        resolution: LockfileResolution::Directory(DirectoryResolution {
            directory: spec.dependency_path,
        }),
        resolved_via: spec.resolved_via,
    }))
}

/// The manifest of the project whose `publishConfig.directory` a `file:`
/// directory is. It describes the directory however the directory was
/// declared, as it does for a `workspace:` range, so a `package.json` a build
/// writes there does not change the resolution. A `link:` directory keeps its
/// own `package.json`.
///
/// Only projects inside the workspace root count, and a manifest that fails
/// to read is no such project, as the lockfile freshness check reads them.
/// A directory that does not exist gets none, so it fails as missing.
fn publishing_project_manifest(
    spec: &LocalPackageSpec,
    opts: &LocalResolverOptions,
) -> Option<serde_json::Value> {
    if !spec.id.as_str().starts_with("file:") || !spec.fetch_spec.is_dir() {
        return None;
    }
    let root = opts.workspace_dir
        .as_deref()
        .or(opts.lockfile_dir.as_deref())
        .unwrap_or(&opts.project_dir);
    let root = pnpm_fs::lexical_normalize(root);
    let target = pnpm_fs::lexical_normalize(&spec.fetch_spec);
    target
        .ancestors()
        .skip(1)
        .take_while(|dir| dir.starts_with(&root))
        .find_map(|dir| {
            let manifest = safe_read_project_manifest_from_dir(dir).ok().flatten()?;
            publishes_from_directory(dir, &manifest, &target).then_some(manifest)
        })
}

/// Decide the fall-back when `package.json` is missing. For `file:`
/// specs (copy-shaped) this throws `ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND` when the
/// directory itself doesn't exist; for `link:` with a missing
/// `package.json` it warns and substitutes a manifest with the
/// directory basename and no version: the version is unknown, so ranged
/// `packageExtensions` and overrides selectors must not match it
/// (<https://github.com/pnpm/pnpm/issues/15007>). The dependency resolver
/// stamps the `0.0.0` identity default after the manifest hooks have run.
/// A missing directory whose `path_shaped_specifier` opens with a protocol
/// fails with `ERR_PNPM_UNSUPPORTED_PROTOCOL` instead, so the protocol is
/// checked only after resolution has already failed.
fn synthesize_fallback_manifest(
    filesystem_policy: LocalFilesystemPolicy,
    spec: &LocalPackageSpec,
    opts: &LocalResolverOptions,
    path_shaped_specifier: Option<&str>,
) -> Result<serde_json::Value, ResolveLocalError> {
    let metadata = std::fs::metadata(&spec.fetch_spec);
    if matches!(&metadata, Err(err) if pnpm_fs::is_not_found(err)) {
        if let Some(err) = path_shaped_specifier.and_then(UnsupportedProtocolError::detect) {
            return Err(ResolveLocalError::UnsupportedProtocol(err));
        }
        if spec.id.as_str().starts_with("file:") {
            return Err(ResolveLocalError::LinkedPkgDirNotFound {
                path: spec.fetch_spec.display().to_string(),
            });
        }
        // Warn via `tracing::warn!` until pacquet's reporter grows a
        // generic `pnpm:logger` channel.
        let prefix = opts.project_dir.display();
        let fetch_spec = spec.fetch_spec.display();
        tracing::warn!(
            target: "pacquet::resolving-local-resolver",
            prefix = %prefix,
            "Installing a dependency from a non-existent directory: {fetch_spec}",
        );
    } else if let Ok(metadata) = metadata
        && !metadata.is_dir()
    {
        // The path exists but isn't a directory (e.g. a `.tgz` that
        // slipped past tarball-shape detection because the spec used
        // the `link:` scheme). On Windows `read(<file>/package.json)`
        // returns `NotFound` rather than `NotADirectory`, so this
        // explicit check surfaces `ERR_PNPM_NOT_PACKAGE_DIRECTORY` on every
        // platform.
        return Err(ResolveLocalError::NotPackageDirectory {
            path: spec.fetch_spec.display().to_string(),
        });
    }
    if let Some(manifest) =
        find_bounded_parent_publish_manifest(filesystem_policy, &spec.fetch_spec, opts)?
    {
        return Ok(manifest);
    }
    let name = spec.fetch_spec
        .file_name()
        .map(OsStr::to_string_lossy)
        .map(std::borrow::Cow::into_owned)
        .unwrap_or_default();
    Ok(serde_json::json!({ "name": name }))
}

/// Map a [`PackageManifestError`] from
/// [`safe_read_package_json_from_dir`] into the resolver's error
/// surface.
fn handle_manifest_read_failure(
    err: PackageManifestError,
    spec: &LocalPackageSpec,
) -> ResolveLocalError {
    if let PackageManifestError::Read { source: io_err, .. } = &err {
        match io_err.kind() {
            std::io::ErrorKind::NotADirectory => {
                return ResolveLocalError::NotPackageDirectory {
                    path: spec.fetch_spec.display().to_string(),
                };
            }
            std::io::ErrorKind::NotFound => {
                // ENOENT normally synthesizes the placeholder manifest,
                // but that path is reached via `Ok(None)` from
                // `safe_read_package_json_from_dir` hitting
                // `synthesize_fallback_manifest` first. Reaching this
                // arm therefore means a directory was renamed mid-read;
                // treat it as a hard failure rather than racing back
                // into the fall-back path.
            }
            _ => {}
        }
    }
    ResolveLocalError::ReadManifest(err)
}

mod filesystem_policy;
mod tarball;
