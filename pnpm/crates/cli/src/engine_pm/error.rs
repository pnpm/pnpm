//! Errors raised while provisioning a package manager. The codes carry the
//! shared `ERR_PNPM_` prefix.

use derive_more::{Display, Error};
use miette::Diagnostic;

use crate::engine_pm::channel::PackageManager;

#[derive(Debug, Display, Error, Diagnostic)]
pub(crate) enum EngineError {
    /// pnpm's own unresolvable-version error predates the other package
    /// managers and is part of the CLI's error-code contract, so it keeps
    /// its own code. [`EngineError::cannot_resolve`] picks between the two.
    #[display(r#"Cannot resolve pnpm version for "{spec}""#)]
    #[diagnostic(code(ERR_PNPM_CANNOT_RESOLVE_PNPM))]
    CannotResolvePnpm { spec: String },

    #[display(r#"Cannot resolve {name} version for "{spec}""#)]
    #[diagnostic(code(ERR_PNPM_CANNOT_RESOLVE_PACKAGE_MANAGER))]
    CannotResolvePackageManager { name: &'static str, spec: String },

    #[display("{name}@{version} is not published to a registry")]
    #[diagnostic(code(ERR_PNPM_ENGINE_NOT_REGISTRY_PUBLISHED))]
    NotRegistryPublished { name: &'static str, version: String },

    #[display("Cannot find the {name} executable in {dir}")]
    #[diagnostic(
        code(ERR_PNPM_ENGINE_BIN_MISSING),
        help(
            "The installed engine is incomplete. Remove that directory and run the command again."
        )
    )]
    MissingEngineBin { name: &'static str, dir: String },

    #[display("The package.json of this project is not a JSON object")]
    #[diagnostic(
        code(ERR_PNPM_INVALID_MANIFEST),
        help("A package.json holds an object. Fix it before declaring a package manager in it.")
    )]
    ManifestIsNotAnObject,

    #[display("Unable to find the global packages directory")]
    #[diagnostic(
        code(ERR_PNPM_NO_GLOBAL_BIN_DIR),
        help(
            r#"Run "pnpm setup" to create it automatically, or set the global-bin-dir setting, or the PNPM_HOME env variable."#
        )
    )]
    NoGlobalDir,
}

impl EngineError {
    pub(crate) fn cannot_resolve(pm: PackageManager, version_spec: &str) -> Self {
        let spec = version_spec.to_string();
        match pm {
            PackageManager::Pnpm => EngineError::CannotResolvePnpm { spec },
            _ => EngineError::CannotResolvePackageManager { name: pm.name(), spec },
        }
    }
}
