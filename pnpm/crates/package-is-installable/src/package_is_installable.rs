//! Composes the engine and platform checks into a tri-state
//! installability verdict.

use crate::{
    check_engine::{
        Engine, InvalidNodeVersionError, UnsupportedEngineError, WantedEngine, check_engine,
    },
    check_platform::{
        SupportedArchitectures, UnsupportedPlatformError, WantedPlatformRef, check_platform,
    },
    infer_platform_from_package_name::inferred_platform,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use serde::Serialize;

/// Inputs from a package manifest (or lockfile metadata row) that
/// drive the installability check.
///
/// `name` feeds the platform-from-name inference for optional
/// dependencies (see [`inferred_platform`]); an empty name disables
/// the inference and leaves only the declared fields.
#[derive(Debug, Default, Clone)]
pub struct PackageInstallabilityManifest {
    pub name: String,
    pub engines: Option<WantedEngine>,
    pub cpu: Option<Vec<String>>,
    pub os: Option<Vec<String>>,
    pub libc: Option<Vec<String>>,
}

/// Discriminator on `pnpm:skipped-optional-dependency` payloads:
/// `'unsupported_engine'` or `'unsupported_platform'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    UnsupportedEngine,
    UnsupportedPlatform,
}

/// Errors [`package_is_installable`] can surface. The first two
/// variants carry an unsupported-engine or unsupported-platform error.
/// The third propagates [`check_engine`]'s `ERR_PNPM_INVALID_NODE_VERSION`
/// failure so callers keep that error code instead of seeing it
/// collapsed into a misleading engine-mismatch error.
#[derive(Debug, Display, Error, Diagnostic, Clone, PartialEq, Eq)]
pub enum InstallabilityError {
    #[display("{_0}")]
    #[diagnostic(transparent)]
    Engine(UnsupportedEngineError),
    #[display("{_0}")]
    #[diagnostic(transparent)]
    Platform(UnsupportedPlatformError),
    #[display("{_0}")]
    #[diagnostic(transparent)]
    InvalidNodeVersion(InvalidNodeVersionError),
}

impl InstallabilityError {
    /// Map the wrapped error variant to its `pnpm:skipped-optional-dependency`
    /// reason (`unsupported_engine` or `unsupported_platform`).
    ///
    /// `InvalidNodeVersion` is treated as an engine-class skip
    /// because it is raised from inside the engine evaluation; the
    /// reason discriminator on a `pnpm:skipped-optional-dependency`
    /// event for an invalid node version is therefore
    /// `unsupported_engine`, even though the underlying error code is
    /// `ERR_PNPM_INVALID_NODE_VERSION`.
    #[must_use]
    pub fn skip_reason(&self) -> SkipReason {
        match self {
            Self::Engine(_) | Self::InvalidNodeVersion(_) => SkipReason::UnsupportedEngine,
            Self::Platform(_) => SkipReason::UnsupportedPlatform,
        }
    }
}

/// Tri-state verdict returned by [`package_is_installable`].
///
/// - [`InstallabilityVerdict::Installable`]: no warning, no skip, just
///   install.
/// - [`InstallabilityVerdict::SkipOptional`]: the package is
///   incompatible and was declared optional; caller should emit
///   `pnpm:skipped-optional-dependency` and exclude the package from
///   the install set.
/// - [`InstallabilityVerdict::ProceedWithWarning`]: the package is
///   incompatible, not optional, and `engineStrict` is off; caller
///   emits `pnpm:install-check` warn (or a tracing-level warning) and
///   proceeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallabilityVerdict {
    Installable,
    SkipOptional {
        reason: SkipReason,
        /// Details string the caller copies into the
        /// `pnpm:skipped-optional-dependency` payload's `details`
        /// field.
        details: String,
    },
    ProceedWithWarning {
        /// Message body for the `pnpm:install-check` warn.
        message: String,
    },
}

/// Options threaded into [`package_is_installable`] / [`check_package`].
///
/// `current_node_version` corresponds to the `nodeVersion` config
/// setting: if present and parseable, it's used as the current node
/// version; if absent, the caller passes the actual runtime's version.
/// `pnpm_version` is normally `None` for pacquet (pacquet isn't pnpm);
/// it can be set from the detected system pnpm version or a config
/// override. `engine_strict` defaults to false, and
/// `supported_architectures` is read from `pnpm-workspace.yaml` when
/// present.
///
/// All string fields borrow so a caller running through many snapshots
/// in a row can build the host-derived part of the struct once and
/// only toggle `optional` per snapshot.
#[derive(Debug, Default, Clone, Copy)]
pub struct InstallabilityOptions<'a> {
    pub engine_strict: bool,
    pub optional: bool,
    pub current_node_version: &'a str,
    pub pnpm_version: Option<&'a str>,
    pub current_os: &'a str,
    pub current_cpu: &'a str,
    pub current_libc: &'a str,
    pub supported_architectures: Option<&'a SupportedArchitectures>,
}

/// Pure compose of [`check_platform`] and [`check_engine`]. Returns
/// the first error a manifest produces, or `None` if compatible.
pub fn check_package(
    package_id: &str,
    manifest: &PackageInstallabilityManifest,
    options: &InstallabilityOptions<'_>,
) -> Result<Option<InstallabilityError>, InvalidNodeVersionError> {
    // Defaulting each absent platform axis to `['any']` is functionally
    // equivalent to "no constraint", since `check_list` short-circuits a
    // single-element `['any']` to accept. `check_platform` already skips
    // an axis when the wanted slice is `None`, so we pass the manifest
    // fields by reference straight through — no `vec!["any".to_string()]`
    // per axis, no `WantedPlatform { ... }` clone of the manifest's owned
    // vectors. The owned `WantedPlatform` only materialises inside
    // `check_platform` when an error is returned.
    let wanted = WantedPlatformRef {
        os: manifest.os.as_deref(),
        cpu: manifest.cpu.as_deref(),
        libc: manifest.libc.as_deref(),
    };
    if let Some(platform_err) = check_platform(
        package_id,
        wanted,
        options.supported_architectures,
        options.current_os,
        options.current_cpu,
        options.current_libc,
    ) {
        return Ok(Some(InstallabilityError::Platform(platform_err)));
    }

    let Some(wanted_engines) = manifest.engines.as_ref() else {
        return Ok(None);
    };

    let current = Engine {
        node: options.current_node_version.to_string(),
        pnpm: options.pnpm_version.map(str::to_string),
    };
    match check_engine(package_id, wanted_engines, &current)? {
        Some(engine_err) => Ok(Some(InstallabilityError::Engine(engine_err))),
        None => Ok(None),
    }
}

/// Produces the tri-state installability verdict.
///
/// Side effects (the `pnpm:install-check` warn and
/// `pnpm:skipped-optional-dependency` emit) are *not* performed here
/// — the caller composes them so log payloads can carry pacquet-
/// specific context (`prefix`, `requester`, etc.).
///
/// [`InstallabilityError`] is large (200+ bytes) because it carries the
/// full wanted/current platform or engine state for diagnostic
/// rendering. Boxing the `Err` arm keeps `Result<_, _>` small enough
/// for clippy's `result-large-err` lint on installs where the error
/// path is rare.
pub fn package_is_installable(
    package_id: &str,
    manifest: &PackageInstallabilityManifest,
    options: &InstallabilityOptions<'_>,
) -> Result<InstallabilityVerdict, Box<InstallabilityError>> {
    let effective: PackageInstallabilityManifest;
    let manifest = if options.optional
        && let Some(platform) = inferred_platform(
            &manifest.name,
            WantedPlatformRef {
                os: manifest.os.as_deref(),
                cpu: manifest.cpu.as_deref(),
                libc: manifest.libc.as_deref(),
            },
        ) {
        effective = PackageInstallabilityManifest {
            name: manifest.name.clone(),
            engines: manifest.engines.clone(),
            os: platform.os,
            cpu: platform.cpu,
            libc: platform.libc,
        };
        &effective
    } else {
        manifest
    };
    let warn = match check_package(package_id, manifest, options) {
        Ok(maybe) => maybe,
        Err(invalid_node) => {
            // `check_engine` fails on an invalid node version
            // regardless of `engineStrict`. Surface as the dedicated
            // `InvalidNodeVersion` variant so callers keep the
            // `ERR_PNPM_INVALID_NODE_VERSION` code and message rather
            // than a synthesized engine mismatch.
            return Err(Box::new(InstallabilityError::InvalidNodeVersion(invalid_node)));
        }
    };
    let Some(warn) = warn else { return Ok(InstallabilityVerdict::Installable) };

    if options.optional {
        return Ok(InstallabilityVerdict::SkipOptional {
            reason: warn.skip_reason(),
            details: warn.to_string(),
        });
    }

    if options.engine_strict {
        return Err(Box::new(warn));
    }

    Ok(InstallabilityVerdict::ProceedWithWarning { message: warn.to_string() })
}
