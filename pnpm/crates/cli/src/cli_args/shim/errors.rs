use super::{Diagnostic, Display, Error};

/// Errors specific to `pacquet shim`. The codes carry the shared
/// `ERR_PNPM_` prefix.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ShimError {
    #[display("Please specify the subcommand")]
    #[diagnostic(
        code(ERR_PNPM_SHIM_NO_SUBCOMMAND),
        help("Usage: pnpm shim add|rm|ls [package...]")
    )]
    NoSubcommand,

    #[display("Unknown subcommand: {subcommand}")]
    #[diagnostic(
        code(ERR_PNPM_SHIM_UNKNOWN_SUBCOMMAND),
        help("Usage: pnpm shim add|rm|ls [package...]")
    )]
    UnknownSubcommand {
        #[error(not(source))]
        subcommand: String,
    },

    #[display("Please specify at least one package")]
    #[diagnostic(code(ERR_PNPM_SHIM_NO_PACKAGE))]
    NoPackage,

    #[display("Unable to find the global bin directory")]
    #[diagnostic(
        code(ERR_PNPM_NO_GLOBAL_BIN_DIR),
        help(
            r#"Run "pnpm setup" to create it automatically, or set the global-bin-dir setting, or the PNPM_HOME env variable."#
        )
    )]
    NoGlobalDir,

    #[display("Cannot create a shim: globalShims is set to false")]
    #[diagnostic(
        code(ERR_PNPM_SHIMS_DISABLED),
        help(
            r#"That setting turns every context-aware shim off, so the shim would sit on PATH doing nothing. Remove it from the global config.yaml, or set "globalShims: true", and add the shim again."#
        )
    )]
    ShimsDisabled,

    #[display("Cannot create a shim for {package}: {bin} is already in the global bin directory")]
    #[diagnostic(
        code(ERR_PNPM_SHIM_BIN_CONFLICT),
        help(
            r#"Another package already provides that command. Remove it with "pnpm remove -g <package>", or remove its shim with "pnpm shim rm <package>"."#
        )
    )]
    BinConflict {
        #[error(not(source))]
        package: String,
        bin: String,
    },

    #[display("Cannot find the bins of {package}")]
    #[diagnostic(
        code(ERR_PNPM_SHIM_NO_BINS),
        help("A shim can only be created for a package that publishes at least one bin.")
    )]
    NoBins {
        #[error(not(source))]
        package: String,
    },
}
