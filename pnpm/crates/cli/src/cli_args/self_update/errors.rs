use super::{Diagnostic, Display, Error};

/// Errors specific to `self-update`. The codes carry the shared
/// `ERR_PNPM_` prefix, so a code already starting with `PNPM_` becomes
/// `ERR_PNPM_PNPM_...`.
#[derive(Debug, Display, Error, Diagnostic)]
pub(crate) enum SelfUpdateError {
    #[display("pnpm cannot update itself when it is executed by Corepack")]
    #[diagnostic(
        code(ERR_PNPM_CANT_SELF_UPDATE_IN_COREPACK),
        help("Install pnpm with the standalone script instead: {install_command}")
    )]
    CantSelfUpdateInCorepack { install_command: &'static str },

    #[display(r#"Cannot find "{specifier}" version of pnpm"#)]
    #[diagnostic(code(ERR_PNPM_CANNOT_RESOLVE_PNPM))]
    CannotResolvePnpm { specifier: String },

    #[display(
        "Refusing to switch to pnpm v{version}: it violates the configured minimumReleaseAge / trustPolicy"
    )]
    #[diagnostic(code(ERR_PNPM_PNPM_RELEASE_POLICY_VIOLATION))]
    ReleasePolicyViolation { version: String },

    #[display("pnpm@{version} {reason}.")]
    #[diagnostic(
        code(ERR_PNPM_NO_MATURE_MATCHING_VERSION),
        help(
            "Wait for the release to mature past the cutoff, or set PNPM_CONFIG_MINIMUM_RELEASE_AGE=0 to update anyway."
        )
    )]
    NoMatureMatchingVersion { version: String, reason: String },

    #[display("Aborted: the immature pnpm version was not approved")]
    #[diagnostic(code(ERR_PNPM_MINIMUM_RELEASE_AGE_DENIED))]
    MinimumReleaseAgeDenied,

    #[diagnostic(code(ERR_PNPM_PNPM_ENGINE_IDENTITY_UNVERIFIABLE))]
    EngineIdentityUnverifiable { message: String },

    #[diagnostic(code(ERR_PNPM_PNPM_ENGINE_IDENTITY_MISMATCH))]
    EngineIdentityMismatch { message: String },

    #[display("Cannot run {label} on this host: it ships no native binary for {target}.")]
    #[diagnostic(
        code(ERR_PNPM_PNPM_ENGINE_NO_NATIVE_BINARY),
        help("Set `pmOnFail` to `ignore` to skip the version switch.")
    )]
    EngineNoNativeBinary { label: String, target: String },

    #[display("Unable to find the global bin directory")]
    #[diagnostic(
        code(ERR_PNPM_NO_GLOBAL_BIN_DIR),
        help(
            r#"Run "pnpm setup" to create it automatically, or set the global-bin-dir setting, or the PNPM_HOME env variable. The global bin directory should be in the PATH."#
        )
    )]
    NoGlobalDir,

    #[display("The pnpm v{version} that was just installed cannot run: {reason}")]
    #[diagnostic(
        code(ERR_PNPM_BROKEN_PNPM_INSTALL),
        help(
            r#"The installation at "{executable}" was discarded and the currently active pnpm was left in place, so pnpm still works. A release that installs but cannot run is a packaging fault — please report it at https://github.com/pnpm/pnpm/issues. To move to a different version meanwhile, pass one to "pnpm self-update"."#
        )
    )]
    BrokenPnpmInstall {
        version: String,
        reason: String,
        executable: String,
    },

    #[display("pnpm v{version} is a broken release and cannot be installed")]
    #[diagnostic(
        code(ERR_PNPM_BROKEN_PNPM_RELEASE),
        help(
            r#"Its "@pnpm/exe" build shipped without a binary and does not run. Even where it does run, pinning it would break everyone on the project who uses "@pnpm/exe", because the pin is shared. Choose another version, or run "pnpm self-update latest"."#
        )
    )]
    BrokenPnpmRelease { version: String },
}
