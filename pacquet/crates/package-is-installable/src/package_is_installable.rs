//! Port of `packageIsInstallable` from
//! <https://github.com/pnpm/pnpm/blob/94240bc046/config/package-is-installable/src/index.ts>.

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

/// Discriminator on `pnpm:skipped-optional-dependency` payloads.
/// Matches upstream's `'unsupported_engine' | 'unsupported_platform'`
/// pair at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/config/package-is-installable/src/index.ts#L57>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    UnsupportedEngine,
    UnsupportedPlatform,
}

/// Errors [`package_is_installable`] can surface. The first two
/// variants mirror upstream's `UnsupportedEngineError |
/// UnsupportedPlatformError` from `index.ts:81`. The third propagates
/// `checkEngine`'s `ERR_PNPM_INVALID_NODE_VERSION` throw at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/config/package-is-installable/src/checkEngine.ts#L25-L27>
/// so callers keep the upstream error code instead of seeing it
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
    /// reason, matching `index.ts:57`'s `'unsupported_engine' |
    /// 'unsupported_platform'` ternary.
    ///
    /// `InvalidNodeVersion` is treated as an engine-class skip
    /// because upstream's `checkEngine` throws it from inside the
    /// engine evaluation; the reason discriminator on a
    /// `pnpm:skipped-optional-dependency` event for an invalid node
    /// version is therefore `unsupported_engine`, even though the
    /// underlying error code is `ERR_PNPM_INVALID_NODE_VERSION`.
    #[must_use]
    pub fn skip_reason(&self) -> SkipReason {
        match self {
            Self::Engine(_) | Self::InvalidNodeVersion(_) => SkipReason::UnsupportedEngine,
            Self::Platform(_) => SkipReason::UnsupportedPlatform,
        }
    }
}

/// Tri-state verdict mirroring upstream's `boolean | null` return at
/// `index.ts:38`. Returned by [`package_is_installable`].
///
/// - [`InstallabilityVerdict::Installable`]: maps to upstream `true`.
///   No warning, no skip, just install.
/// - [`InstallabilityVerdict::SkipOptional`]: maps to upstream `false`.
///   The package is incompatible and was declared optional; caller
///   should emit `pnpm:skipped-optional-dependency` and exclude the
///   package from the install set.
/// - [`InstallabilityVerdict::ProceedWithWarning`]: maps to upstream
///   `null`. The package is incompatible, not optional, and
///   `engineStrict` is off; caller emits `pnpm:install-check` warn
///   (or a tracing-level warning) and proceeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallabilityVerdict {
    Installable,
    SkipOptional {
        reason: SkipReason,
        /// Details string the caller copies into the
        /// `pnpm:skipped-optional-dependency` payload's `details`
        /// field. Matches upstream `warn.toString()` at `index.ts:50`.
        details: String,
    },
    ProceedWithWarning {
        /// Message body for the `pnpm:install-check` warn. Matches
        /// upstream `warn.message` at `index.ts:44`.
        message: String,
    },
}

/// Options threaded into [`package_is_installable`] / [`check_package`].
///
/// `current_node_version` mirrors pnpm's `nodeVersion` config setting:
/// if present and parseable, it's used as the current node version;
/// if absent, the caller passes the actual runtime's version.
/// `pnpm_version` is normally `None` for pacquet (pacquet isn't pnpm);
/// upstream sets this from `getSystemPnpmVersion()` or a config
/// override. `engine_strict` defaults to false (pnpm's default at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/index.ts>),
/// and `supported_architectures` is read from `pnpm-workspace.yaml`
/// when present.
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
/// Mirrors upstream `checkPackage` at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/config/package-is-installable/src/index.ts#L68-L94>.
///
/// Platform is checked first (so an unsupported OS surfaces as a
/// `Platform` error even if the engine range would also reject).
pub fn check_package(
    package_id: &str,
    manifest: &PackageInstallabilityManifest,
    options: &InstallabilityOptions<'_>,
) -> Result<Option<InstallabilityError>, InvalidNodeVersionError> {
    // Pacquet-only optimization (still wire-compatible with upstream):
    // upstream's `index.ts:82-86` defaults each absent platform axis
    // to `['any']` before passing it down to `checkPlatform`. That
    // shape is functionally equivalent to "no constraint", since
    // `checkList` short-circuits a single-element `['any']` to
    // accept. Pacquet's `check_platform` already skips an axis when
    // the wanted slice is `None`, so we pass the manifest fields by
    // reference straight through — no `vec!["any".to_string()]` per
    // axis, no `WantedPlatform { ... }` clone of the manifest's
    // owned vectors. The owned `WantedPlatform` only materialises
    // inside `check_platform` when an error is returned.
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

/// Tri-state installability verdict, mirroring upstream
/// `packageIsInstallable` at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/config/package-is-installable/src/index.ts#L20-L66>.
///
/// Side effects (the `pnpm:install-check` warn and
/// `pnpm:skipped-optional-dependency` emit) are *not* performed here
/// — the caller composes them so log payloads can carry pacquet-
/// specific context (`prefix`, `requester`, etc.).
///
/// `InstallabilityError` is large (200+ bytes) because it carries the
/// full wanted/current platform or engine state for diagnostic
/// rendering. Boxing the `Err` arm keeps `Result<_, _>` small enough
/// for clippy's `result-large-err` lint on installs where the error
/// path is rare.
pub fn package_is_installable(
    package_id: &str,
    manifest: &PackageInstallabilityManifest,
    options: &InstallabilityOptions<'_>,
) -> Result<InstallabilityVerdict, Box<InstallabilityError>> {
    // Mirrors upstream's `effectivePlatform(pkg, options.optional)` at
    // <https://github.com/pnpm/pnpm/blob/34875b2d7c/config/package-is-installable/src/index.ts#L41>:
    // an optional dependency with incomplete platform fields gets the
    // missing ones filled from its name before the check runs.
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
            // Upstream `checkEngine` `throw`s on invalid node version
            // regardless of `engineStrict`. Surface as the dedicated
            // `InvalidNodeVersion` variant so callers keep the
            // `ERR_PNPM_INVALID_NODE_VERSION` code and message
            // upstream uses, rather than a synthesized engine
            // mismatch.
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
