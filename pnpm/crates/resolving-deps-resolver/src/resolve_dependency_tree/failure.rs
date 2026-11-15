use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_catalogs_resolver::CatalogResolutionError;
use pnpm_network::redact_and_sanitize;
use pnpm_patching::PatchKeyConflictError;
use pnpm_resolving_npm_resolver::PickPackageError;
use pnpm_resolving_resolver_base::{
    GitResolveError, NoMatchingVersionError, RegistryResponseError, ResolveError,
    UnsupportedProtocolError,
};
use std::{fmt, sync::Arc};

use super::SkippedOptionalDependencyParent;

/// Cooperative cancellation probe polled during synchronous and multi-pass
/// dependency resolution stages.
#[derive(Default, Clone)]
pub struct ResolutionCancel(Option<Arc<dyn Fn() -> bool + Send + Sync>>);

impl fmt::Debug for ResolutionCancel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ResolutionCancel")
            .field(&self.0.as_ref().map(|_| ".."))
            .finish()
    }
}

impl ResolutionCancel {
    #[must_use]
    pub fn new(check: Arc<dyn Fn() -> bool + Send + Sync>) -> Self {
        Self(Some(check))
    }

    #[must_use]
    pub fn from_option(check: Option<Arc<dyn Fn() -> bool + Send + Sync>>) -> Self {
        Self(check)
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.as_ref().is_some_and(|check| check())
    }

    #[must_use]
    pub fn is_configured(&self) -> bool {
        self.0.is_some()
    }
}

/// The failed edge and the packages that led to it. The source retains its
/// diagnostic code and help, including failures of locked optional edges.
#[derive(Debug, Error, Diagnostic)]
#[diagnostic(forward(source))]
pub struct DependencyResolutionError {
    pub alias: String,
    pub specifier: String,
    pub parents: Vec<SkippedOptionalDependencyParent>,
    pub prefix: String,
    #[error(source)]
    pub source: Box<ResolveDependencyTreeError>,
}

impl fmt::Display for DependencyResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}\n\nFailed to resolve {}@{}",
            self.source,
            redact_and_sanitize(&self.alias),
            redact_and_sanitize(&self.specifier),
        )?;
        for parent in &self.parents {
            write!(
                formatter,
                "\nThis error happened while installing the dependencies of {}@{}",
                redact_and_sanitize(&parent.name),
                redact_and_sanitize(&parent.version),
            )?;
        }
        Ok(())
    }
}

/// Error envelope returned by the tree walker.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ResolveDependencyTreeError {
    #[diagnostic(transparent)]
    DependencyContext(#[error(source)] DependencyResolutionError),

    /// One of the resolver chain calls failed (network, parse, etc.).
    /// The inner error is the boxed type the resolver returned.
    #[display("Failed to resolve dependency: {_0}")]
    Resolve(#[error(source)] ResolveError),

    #[display("Conflicting registry revisions were requested for \"{name}@{version}\".")]
    #[diagnostic(
        code(ERR_PNPM_REVISION_CONFLICT),
        help(
            "A single package name and version can resolve to only one registry artifact in an install."
        )
    )]
    RevisionConflict { name: String, version: String },

    /// The registry publishes the package but nothing the request accepts,
    /// raised with the `ERR_PNPM_NO_MATCHING_VERSION` code.
    #[diagnostic(transparent)]
    NoMatchingVersion(#[error(source)] NoMatchingVersionError),

    /// The registry answered the metadata request with a non-2xx status,
    /// raised with the matching `ERR_PNPM_FETCH_<status>` code.
    #[diagnostic(transparent)]
    RegistryResponse(#[error(source)] RegistryResponseError),

    /// A git dependency's `git ls-remote` failed, raised with the
    /// `ERR_PNPM_GIT_RESOLVE_FAILED` code.
    #[diagnostic(transparent)]
    GitResolve(#[error(source)] GitResolveError),

    /// A specifier opens with a protocol no resolver supports, raised with
    /// the `ERR_PNPM_UNSUPPORTED_PROTOCOL` code.
    #[diagnostic(transparent)]
    UnsupportedProtocol(#[error(source)] UnsupportedProtocolError),

    /// The npm resolver's cache/fetch orchestration failed — most often
    /// `ERR_PNPM_NO_OFFLINE_META`, raised with whatever code and help
    /// [`PickPackageError`] itself carries.
    #[diagnostic(transparent)]
    Pick(#[error(source)] PickPackageError),

    /// An optional dependency failed to resolve while the wanted
    /// lockfile still holds a package entry satisfying the wanted
    /// range. Rethrown loudly instead of skipped, because skipping
    /// would erase the locked entries and make the lockfile differ
    /// depending on which machine ran the install
    /// (<https://github.com/pnpm/pnpm/issues/12853>).
    #[diagnostic(
        forward(0),
        help(
            "This optional dependency is not skipped, because the lockfile contains a resolution for it. Skipping it would remove the locked entries, making the lockfile differ depending on which machine ran the install. If the version was intentionally removed from the registry, update the dependent package or remove the entries from the lockfile."
        )
    )]
    LockedOptionalResolutionFailure(#[error(source)] Box<ResolveDependencyTreeError>),

    /// No resolver in the chain claimed the spec, raised with the
    /// `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER` code.
    #[display("\"{specifier}\" isn't supported by any available resolver.")]
    #[diagnostic(code(ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER))]
    SpecNotSupported {
        #[error(not(source))]
        specifier: String,
    },

    /// A `catalog:` specifier on a direct dependency referenced a
    /// missing entry, used a forbidden protocol, or was otherwise
    /// misconfigured. The inner error carries the
    /// `ERR_PNPM_CATALOG_ENTRY_*` code and message.
    #[diagnostic(transparent)]
    CatalogMisconfiguration(#[error(source)] CatalogResolutionError),

    /// `patchedDependencies` configured more than one version range that
    /// satisfies the same `name@version` and the user did not break the
    /// tie with an exact-version entry. Propagated verbatim from
    /// [`pnpm_patching::get_patch_info`].
    #[diagnostic(transparent)]
    PatchKeyConflict(#[error(source)] PatchKeyConflictError),

    /// A transitive dependency was resolved through an exotic
    /// protocol (git, tarball, file, ...) while `block_exotic_subdeps`
    /// is on, raised with the `ERR_PNPM_EXOTIC_SUBDEP` code.
    #[display(
        "Exotic dependency \"{specifier}\" (resolved via {resolved_via}) is not allowed in subdependencies when blockExoticSubdeps is enabled"
    )]
    #[diagnostic(code(ERR_PNPM_EXOTIC_SUBDEP))]
    ExoticSubdep {
        #[error(not(source))]
        specifier: String,
        resolved_via: String,
    },

    /// A dependency alias contained a path-separator segment that would
    /// escape the intended `node_modules` directory when joined onto a
    /// modules path, raised with the `ERR_PNPM_INVALID_DEPENDENCY_NAME` code.
    #[display(
        "{parent} contains a dependency with an invalid name: {alias:?}. Dependency names must be a single package name or \"@scope/name\" — they cannot contain path-separator segments such as \"..\"."
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_DEPENDENCY_NAME))]
    InvalidDependencyName {
        #[error(not(source))]
        parent: String,
        alias: String,
    },

    #[diagnostic(code(ERR_PNPM_PNPMFILE_FAIL))]
    PnpmfileHook(#[error(not(source))] pnpm_hooks::HookError),

    #[diagnostic(code(ERR_PNPM_BAD_READ_PACKAGE_HOOK_RESULT))]
    BadReadPackageHookResult(#[error(not(source))] pnpm_hooks::HookError),

    /// An importer's `peerDependencies` entry held a value that is neither a
    /// peer range nor a scheme-carrying specifier, raised with the
    /// `ERR_PNPM_INVALID_PEER_DEPENDENCY_SPECIFICATION` code.
    #[display(
        "The peerDependencies field named '{dep_name}' of package '{project_id}' has an invalid value: '{specifier}'"
    )]
    #[diagnostic(
        code(ERR_PNPM_INVALID_PEER_DEPENDENCY_SPECIFICATION),
        help(
            "The values in peerDependencies should be a valid semver range, a `workspace:`/`catalog:` spec, or a dependency specifier such as a named-registry (`<registry>:<version>`), `npm:`, `file:`, or git/URL spec"
        )
    )]
    InvalidPeerDependencySpecification {
        #[error(not(source))]
        dep_name: String,
        project_id: String,
        specifier: String,
    },

    #[display("dependency graph exceeds maximum of {max_snapshots} package snapshots")]
    #[diagnostic(code(ERR_PNPR_GRAPH_TOO_LARGE))]
    SnapshotLimitExceeded {
        #[error(not(source))]
        max_snapshots: usize,
    },

    #[display("resolution timed out")]
    #[diagnostic(code(ERR_PNPR_RESOLVE_TIMEOUT))]
    ResolutionCancelled,
}

impl From<PatchKeyConflictError> for ResolveDependencyTreeError {
    fn from(err: PatchKeyConflictError) -> Self {
        ResolveDependencyTreeError::PatchKeyConflict(err)
    }
}

impl From<pnpm_hooks::HookError> for ResolveDependencyTreeError {
    fn from(err: pnpm_hooks::HookError) -> Self {
        match err {
            pnpm_hooks::HookError::BadReadPackageResult { .. } => {
                Self::BadReadPackageHookResult(err)
            }
            _ => Self::PnpmfileHook(err),
        }
    }
}
