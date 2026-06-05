//! Port of pnpm's
//! [`resolving/local-resolver/src/index.ts`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/index.ts).
//!
//! Three free functions match upstream's exported entry points; they
//! all funnel into [`resolve_spec`], which handles the tarball
//! integrity / directory manifest reading once a [`LocalPackageSpec`]
//! has been chosen.

use std::path::PathBuf;

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_lockfile::{DirectoryResolution, LockfileResolution, TarballResolution};
use pacquet_package_manifest::{PackageManifestError, safe_read_package_json_from_dir};
use pacquet_resolving_resolver_base::{LatestInfo, LatestQuery, PkgResolutionId, ResolveResult};
use ssri::{Algorithm, Integrity, IntegrityOpts};

use crate::parse_bare_specifier::{
    LocalPackageSpec, LocalSpecKind, ParseOptions, PathProtocolNotSupportedError,
    WantedLocalDependency, parse_local_path, parse_local_scheme,
};

/// Per-install knobs the dispatcher threads into every resolver call.
/// Mirrors upstream's
/// [`LocalResolverContext`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/index.ts#L22-L24).
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalResolverContext {
    /// When `true`, an absolute path in the wanted specifier is
    /// preserved in the resolved `id` rather than being relativised
    /// against the project / lockfile root. Mirrors pnpm's
    /// `preserveAbsolutePaths` config (off by default).
    pub preserve_absolute_paths: bool,
}

/// Per-call options. Mirrors upstream's
/// [`LocalResolverOptions`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/index.ts#L26-L34).
#[derive(Debug, Clone)]
pub struct LocalResolverOptions {
    pub project_dir: PathBuf,
    /// Lockfile root. Defaults to `project_dir` when `None` — mirrors
    /// upstream's `opts.lockfileDir ?? opts.projectDir` fallback.
    pub lockfile_dir: Option<PathBuf>,
    /// Previously resolved entry from the lockfile, threaded so the
    /// resolver can short-circuit directory resolution when the
    /// install isn't asking for an update. Mirrors upstream's
    /// `currentPkg` field.
    pub current_pkg: Option<LocalCurrentPkg>,
    /// `false` keeps the lockfile pin. Mirrors upstream's
    /// `update?: false | 'compatible' | 'latest'` tri-state, but
    /// pacquet collapses the two truthy values because the local
    /// resolver only branches on truthy / falsy.
    pub update: LocalResolverUpdate,
}

/// Lockfile-pinned slice the local resolver short-circuits on for
/// directory deps when no update is requested. Mirrors upstream's
/// inline `currentPkg?.{ id, resolution }`.
#[derive(Debug, Clone)]
pub struct LocalCurrentPkg {
    pub id: PkgResolutionId,
    pub resolution: LockfileResolution,
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

/// Outcome of a successful local resolve. Mirrors upstream's
/// [`LocalResolveResult`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/index.ts#L15-L20).
#[derive(Debug, Clone)]
pub struct LocalResolveResult {
    pub id: PkgResolutionId,
    pub manifest: Option<std::sync::Arc<serde_json::Value>>,
    pub normalized_bare_specifier: Option<String>,
    pub resolution: LockfileResolution,
    /// `local-filesystem` — the same `resolvedVia` tag upstream uses
    /// across every shape this resolver produces.
    pub resolved_via: &'static str,
}

impl From<LocalResolveResult> for ResolveResult {
    fn from(result: LocalResolveResult) -> Self {
        ResolveResult {
            id: result.id,
            // Local resolutions don't have a `name@version` shape —
            // the canonical name lives in the fetched manifest, not
            // the resolver-time signal. Leave `name_ver` empty so
            // downstream consumers fall back to reading
            // `result.manifest`.
            name_ver: None,
            latest: None,
            published_at: None,
            manifest: result.manifest,
            resolution: result.resolution,
            resolved_via: result.resolved_via.to_string(),
            normalized_bare_specifier: result.normalized_bare_specifier,
            alias: None,
            policy_violation: None,
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

/// Aggregate error type returned by the three public entry points.
/// Mirrors the three branches upstream's `resolveSpec` raises.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ResolveLocalError {
    /// The wanted specifier carries an unsupported scheme. Today only
    /// `path:`; reserved for future additions.
    Spec(#[error(source)] LocalSpecError),

    /// `file:` directory or tarball points at a path that doesn't
    /// exist. Mirrors pnpm's
    /// [`LINKED_PKG_DIR_NOT_FOUND`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/index.ts#L111-L114).
    #[display("Could not install from \"{path}\" as it does not exist.")]
    #[diagnostic(code(LINKED_PKG_DIR_NOT_FOUND))]
    LinkedPkgDirNotFound {
        #[error(not(source))]
        path: String,
    },

    /// `<spec.fetchSpec>` exists but isn't a directory (ENOTDIR).
    /// Mirrors pnpm's
    /// [`NOT_PACKAGE_DIRECTORY`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/index.ts#L127-L129).
    #[display("Could not install from \"{path}\" as it is not a directory.")]
    #[diagnostic(code(NOT_PACKAGE_DIRECTORY))]
    NotPackageDirectory {
        #[error(not(source))]
        path: String,
    },

    /// Tarball integrity computation failed for a `file:` spec.
    Integrity(#[error(source)] std::io::Error),

    /// Reading `<spec.fetchSpec>/package.json` raised something the
    /// resolver doesn't have a specific code for (malformed JSON,
    /// permission denied, ...).
    ReadManifest(#[error(source)] PackageManifestError),
}

/// Resolve a wanted dep declared with an explicit local scheme.
/// Mirrors pnpm's
/// [`resolveFromLocalScheme`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/index.ts#L40-L49).
pub async fn resolve_from_local_scheme(
    ctx: &LocalResolverContext,
    wanted_dependency: &WantedLocalDependency,
    opts: &LocalResolverOptions,
) -> Result<Option<LocalResolveResult>, ResolveLocalError> {
    let project_dir = opts.project_dir.as_path();
    let lockfile_dir = opts.lockfile_dir.as_deref().unwrap_or(project_dir);
    let parse_opts = ParseOptions { preserve_absolute_paths: ctx.preserve_absolute_paths };
    let spec = match parse_local_scheme(wanted_dependency, project_dir, lockfile_dir, parse_opts) {
        Ok(maybe) => maybe,
        Err(err) => {
            return Err(ResolveLocalError::Spec(LocalSpecError::PathProtocolNotSupported(err)));
        }
    };
    resolve_spec(spec, opts).await
}

/// Resolve a wanted dep by path shape alone — no scheme prefix.
/// Mirrors pnpm's
/// [`resolveFromLocalPath`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/index.ts#L51-L65).
pub async fn resolve_from_local_path(
    ctx: &LocalResolverContext,
    wanted_dependency: &WantedLocalDependency,
    opts: &LocalResolverOptions,
) -> Result<Option<LocalResolveResult>, ResolveLocalError> {
    let project_dir = opts.project_dir.as_path();
    let lockfile_dir = opts.lockfile_dir.as_deref().unwrap_or(project_dir);
    let parse_opts = ParseOptions { preserve_absolute_paths: ctx.preserve_absolute_paths };
    let spec = parse_local_path(wanted_dependency, project_dir, lockfile_dir, parse_opts);
    resolve_spec(spec, opts).await
}

/// Latest-version companion. Mirrors pnpm's
/// [`resolveLatestFromLocal`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/index.ts#L67-L77):
/// claims `link:` / `file:` / `workspace:` specs with an empty
/// [`LatestInfo`] so the dispatcher stops there instead of routing
/// the dep into a user-configured named-registry alias of the same
/// name.
#[must_use]
pub fn resolve_latest_from_local(query: &LatestQuery) -> Option<LatestInfo> {
    let bare = query.wanted_dependency.bare_specifier.as_deref()?;
    if bare.starts_with("link:") || bare.starts_with("file:") || bare.starts_with("workspace:") {
        return Some(LatestInfo::default());
    }
    None
}

async fn resolve_spec(
    spec: Option<LocalPackageSpec>,
    opts: &LocalResolverOptions,
) -> Result<Option<LocalResolveResult>, ResolveLocalError> {
    let Some(spec) = spec else {
        return Ok(None);
    };

    if matches!(spec.kind, LocalSpecKind::File) {
        // A missing tarball file raises the same `LINKED_PKG_DIR_NOT_FOUND`
        // code the directory branch uses for a missing `file:` target —
        // matches upstream's behavior where `getTarballIntegrity` raises
        // ENOENT and the same catch in
        // [`resolveSpec`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/index.ts#L108-L141)
        // routes both kinds of missing `file:` target through one
        // pnpm-compatible error code.
        let integrity = match compute_tarball_integrity(&spec.fetch_spec).await {
            Ok(integrity) => integrity,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(ResolveLocalError::LinkedPkgDirNotFound {
                    path: spec.fetch_spec.display().to_string(),
                });
            }
            Err(err) => return Err(ResolveLocalError::Integrity(err)),
        };
        return Ok(Some(LocalResolveResult {
            id: spec.id.clone(),
            manifest: None,
            normalized_bare_specifier: Some(spec.normalized_bare_specifier),
            resolution: LockfileResolution::Tarball(TarballResolution {
                tarball: spec.id.as_str().to_string(),
                integrity: Some(integrity),
                git_hosted: None,
                path: None,
            }),
            resolved_via: "local-filesystem",
        }));
    }

    // Directory branch. Short-circuit when the lockfile already has
    // a pin and the install isn't asking for an update — mirrors
    // upstream's
    // [`opts.currentPkg.resolution && spec.type === 'directory' && !opts.update`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/index.ts#L99-L104).
    if let Some(current) = &opts.current_pkg
        && opts.update == LocalResolverUpdate::Off
    {
        return Ok(Some(LocalResolveResult {
            id: current.id.clone(),
            manifest: None,
            normalized_bare_specifier: Some(spec.normalized_bare_specifier),
            resolution: current.resolution.clone(),
            resolved_via: "local-filesystem",
        }));
    }

    let manifest = match safe_read_package_json_from_dir(&spec.fetch_spec) {
        Ok(Some(manifest)) => manifest,
        Ok(None) => synthesize_fallback_manifest(&spec, opts)?,
        Err(err) => return Err(handle_manifest_read_failure(err, &spec)),
    };

    Ok(Some(LocalResolveResult {
        id: spec.id.clone(),
        manifest: Some(std::sync::Arc::new(manifest)),
        normalized_bare_specifier: Some(spec.normalized_bare_specifier),
        resolution: LockfileResolution::Directory(DirectoryResolution {
            directory: spec.dependency_path,
        }),
        resolved_via: "local-filesystem",
    }))
}

/// Decide the fall-back when `package.json` is missing. For `file:`
/// specs (copy-shaped) upstream throws `LINKED_PKG_DIR_NOT_FOUND` when
/// the directory itself doesn't exist; for `link:` and missing
/// `package.json` it warns and substitutes a manifest with the
/// directory basename and `version: '0.0.0'`. Mirrors upstream's
/// [`existsSync(spec.fetchSpec)` branch](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/local-resolver/src/index.ts#L108-L141).
fn synthesize_fallback_manifest(
    spec: &LocalPackageSpec,
    opts: &LocalResolverOptions,
) -> Result<serde_json::Value, ResolveLocalError> {
    let metadata = std::fs::metadata(&spec.fetch_spec);
    if matches!(&metadata, Err(err) if err.kind() == std::io::ErrorKind::NotFound) {
        if spec.id.as_str().starts_with("file:") {
            return Err(ResolveLocalError::LinkedPkgDirNotFound {
                path: spec.fetch_spec.display().to_string(),
            });
        }
        // Match upstream's `logger.warn({ message, prefix })` emit
        // via `tracing::warn!` until pacquet's reporter grows a
        // generic `pnpm:logger` channel. Same payload shape.
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
        // the `link:` scheme). Upstream raises ENOTDIR explicitly on
        // Windows — where `read(<file>/package.json)` returns
        // `NotFound` rather than `NotADirectory` — inside
        // [`readProjectManifestOnly`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/workspace/project-manifest-reader/src/index.ts#L100-L114).
        // Pacquet does the equivalent check here so the resolver
        // surfaces `NOT_PACKAGE_DIRECTORY` on every platform.
        return Err(ResolveLocalError::NotPackageDirectory {
            path: spec.fetch_spec.display().to_string(),
        });
    }
    let name = spec
        .fetch_spec
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(serde_json::json!({ "name": name, "version": "0.0.0" }))
}

/// Map a [`PackageManifestError`] from
/// [`safe_read_package_json_from_dir`] into the resolver's error
/// surface. Upstream's catch block dispatches on the inner code:
/// `ENOTDIR` → `NOT_PACKAGE_DIRECTORY`, `ENOENT` →
/// fall-back manifest, anything else → re-throw.
fn handle_manifest_read_failure(
    err: PackageManifestError,
    spec: &LocalPackageSpec,
) -> ResolveLocalError {
    if let PackageManifestError::Io(io_err) = &err {
        match io_err.kind() {
            std::io::ErrorKind::NotADirectory => {
                return ResolveLocalError::NotPackageDirectory {
                    path: spec.fetch_spec.display().to_string(),
                };
            }
            std::io::ErrorKind::NotFound => {
                // Mirrors upstream's ENOENT fall-through: synthesize
                // the placeholder manifest. The caller is responsible
                // for wrapping this back into a `ResolveLocalError`
                // since the public API surface is fallible only.
                // Handled by `synthesize_fallback_manifest` already
                // — Ok(None) from `safe_read_package_json_from_dir`
                // hits that branch first, so reaching this arm means
                // a directory was renamed mid-read; pacquet treats it
                // as a hard failure rather than racing back into the
                // fall-back path.
            }
            _ => {}
        }
    }
    ResolveLocalError::ReadManifest(err)
}

/// Port of pnpm's
/// [`getTarballIntegrity`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/crypto/hash/src/index.ts#L40-L42)
/// — computes the SSRI integrity hash for a local tarball. Upstream
/// streams the file through ssri; pacquet reads the whole file and
/// feeds it to ssri's incremental hasher in one shot. Tarballs in the
/// `file:` install path are typically a few MB so the simpler shape
/// has no measurable cost.
async fn compute_tarball_integrity(path: &std::path::Path) -> std::io::Result<Integrity> {
    let bytes = tokio::fs::read(path).await?;
    let mut opts = IntegrityOpts::new().algorithm(Algorithm::Sha512);
    opts.input(&bytes);
    Ok(opts.result())
}
