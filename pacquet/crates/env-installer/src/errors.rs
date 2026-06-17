use derive_more::{Display, Error};
use pacquet_diagnostics::miette::{self, Diagnostic};
use pacquet_lockfile::{LoadLockfileError, SaveLockfileError};
use pacquet_package_manager::ImportIndexedDirError;
use pacquet_resolving_resolver_base::ResolveError;
use pacquet_tarball::TarballError;
use std::{io, path::PathBuf};

/// Errors surfaced while resolving or installing configurational
/// dependencies. The user-facing `code(...)` values mirror the
/// `ERR_PNPM_*` codes pnpm's env-installer throws — they are part of
/// the public contract (<https://pnpm.io/errors>).
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ConfigDepError {
    /// Mirrors pnpm's `CONFIG_DEP_NO_INTEGRITY` at
    /// <https://github.com/pnpm/pnpm/blob/31858c544b/installing/env-installer/src/parseIntegrity.ts>.
    #[display(r#"Your config dependency called "{name}" doesn't have an integrity checksum"#)]
    #[diagnostic(code(ERR_PNPM_CONFIG_DEP_NO_INTEGRITY))]
    NoIntegrity { name: String },

    /// Mirrors pnpm's `CONFIG_DEP_OPTIONAL_NOT_EXACT`.
    #[display(
        r#"Cannot install "{subdep_name}@{spec}" as an optionalDependency of config dependency "{parent_name}": only exact versions are supported (got "{spec}")"#
    )]
    #[diagnostic(code(ERR_PNPM_CONFIG_DEP_OPTIONAL_NOT_EXACT))]
    OptionalNotExact { parent_name: String, subdep_name: String, spec: String },

    /// Mirrors pnpm's `BAD_CONFIG_DEP`.
    #[display("{message}")]
    #[diagnostic(code(ERR_PNPM_BAD_CONFIG_DEP))]
    BadConfigDep { message: String },

    /// Mirrors pnpm's `FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE`.
    #[display("{message}")]
    #[diagnostic(code(ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE))]
    FrozenLockfileOutdated { message: String },

    /// Mirrors pnpm's `ENV_LOCKFILE_CORRUPTED`.
    #[display("{message}")]
    #[diagnostic(code(ERR_PNPM_ENV_LOCKFILE_CORRUPTED))]
    EnvLockfileCorrupted { message: String },

    /// Mirrors pnpm's `INVALID_DEPENDENCY_NAME`, thrown by
    /// [`assertValidDependencyAliases`](https://github.com/pnpm/pnpm/blob/main/installing/deps-resolver/src/validateDependencyAlias.ts).
    #[display(r"{description} contains a dependency with an invalid name: {name:?}")]
    #[diagnostic(
        code(ERR_PNPM_INVALID_DEPENDENCY_NAME),
        help(
            "A dependency name must be a valid npm package name — a single `name` or `@scope/name` \
             consisting of URL-friendly characters, with no leading `.` or `_`, and not equal to \
             reserved names such as `node_modules`."
        )
    )]
    InvalidDependencyName { description: String, name: String },

    /// A config-dependency version is a store path segment
    /// (`<name>/<version>/<hash>`), so a non-semver value is rejected to keep a
    /// traversal-shaped version from escaping the store root.
    #[display(r#"The config dependency "{name}" has an invalid version "{version}""#)]
    #[diagnostic(
        code(ERR_PNPM_INVALID_CONFIG_DEP_VERSION),
        help("A config dependency version must be an exact semver version.")
    )]
    InvalidConfigDepVersion { name: String, version: String },

    #[display("Failed to resolve config dependency {spec}: {error}")]
    #[diagnostic(code(ERR_PNPM_BAD_CONFIG_DEP))]
    Resolve {
        spec: String,
        #[error(source)]
        error: ResolveError,
    },

    #[diagnostic(transparent)]
    ReadLockfile(#[error(source)] LoadLockfileError),

    #[diagnostic(transparent)]
    WriteLockfile(#[error(source)] SaveLockfileError),

    // Transparent so the inner `TarballError`'s pnpm-compatible
    // diagnostic code/message is preserved rather than overridden by a
    // crate-local code.
    #[diagnostic(transparent)]
    DownloadTarball(#[error(source)] TarballError),

    #[diagnostic(transparent)]
    Import(#[error(source)] ImportIndexedDirError),

    #[display("Failed to create config-dependency symlink at {path:?}: {error}")]
    #[diagnostic(code(pacquet_env_installer::symlink))]
    Symlink {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[display("Failed to read config-modules directory {path:?}: {error}")]
    #[diagnostic(code(pacquet_env_installer::read_config_modules))]
    ReadConfigModules {
        path: PathBuf,
        #[error(source)]
        error: io::Error,
    },
}
