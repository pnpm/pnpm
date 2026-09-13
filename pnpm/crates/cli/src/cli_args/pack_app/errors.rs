use super::{Diagnostic, Display, Error};

/// Errors raised by `pacquet pack-app`.
///
/// The codes mirror pnpm's `PnpmError('PACK_APP_*', …)` (which prepends
/// `ERR_PNPM_`) so log consumers parse identical strings.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PackAppError {
    #[display(
        r#""pnpm pack-app" requires a CJS entry file — pass --entry <path> or set "pnpm.app.entry" in package.json."#
    )]
    #[diagnostic(code(ERR_PNPM_PACK_APP_MISSING_ENTRY))]
    MissingEntry,

    #[display("Entry file not found: {path}")]
    #[diagnostic(code(ERR_PNPM_PACK_APP_ENTRY_NOT_FOUND))]
    EntryNotFound {
        #[error(not(source))]
        path: String,
    },

    #[display("Entry path must be a regular file: {path}")]
    #[diagnostic(code(ERR_PNPM_PACK_APP_ENTRY_NOT_FILE))]
    EntryNotFile {
        #[error(not(source))]
        path: String,
    },

    #[display(r#"The entry path "{path}" resolves outside the project directory."#)]
    #[diagnostic(
        code(ERR_PNPM_PACK_APP_ENTRY_OUTSIDE_PROJECT),
        help(
            r#"The entry must be a relative path inside the project directory, not an absolute path or one that escapes via ".."."#
        )
    )]
    EntryOutsideProject {
        #[error(not(source))]
        path: String,
    },

    #[display(r#"The output directory "{path}" resolves outside the project directory."#)]
    #[diagnostic(
        code(ERR_PNPM_PACK_APP_OUTPUT_DIR_OUTSIDE_PROJECT),
        help(
            r#"The output directory must be a relative path inside the project directory, not an absolute path or one that escapes via ".."."#
        )
    )]
    OutputDirOutsideProject {
        #[error(not(source))]
        path: String,
    },

    #[display(
        r#"The output file "{path}" already exists and is not a regular file (e.g. a symlink); refusing to write through it."#
    )]
    #[diagnostic(
        code(ERR_PNPM_PACK_APP_OUTPUT_FILE_NOT_REGULAR),
        help("Remove the existing path, or choose a different --output-name or --output-dir.")
    )]
    OutputFileNotRegular {
        #[error(not(source))]
        path: String,
    },

    #[display(
        r#""pnpm pack-app" requires at least one target — pass --target <triplet> or set "pnpm.app.targets" in package.json. Supported: {supported}"#
    )]
    #[diagnostic(code(ERR_PNPM_PACK_APP_MISSING_TARGET))]
    MissingTarget {
        #[error(not(source))]
        supported: &'static str,
    },

    #[display(
        r#"Invalid target: "{raw}". Expected format: <os>-<arch>[-<libc>] where <os> is {supported_os}, <arch> is x64|arm64, optional <libc> is musl (linux only)."#
    )]
    #[diagnostic(code(ERR_PNPM_PACK_APP_INVALID_TARGET))]
    InvalidTarget { raw: String, supported_os: String },

    #[display(r#"The "musl" libc suffix is only valid for linux targets (got "{raw}")."#)]
    #[diagnostic(code(ERR_PNPM_PACK_APP_INVALID_TARGET))]
    MuslOnNonLinux {
        #[error(not(source))]
        raw: String,
    },

    #[display(
        r#"Invalid runtime "{spec}". Expected format: <name>@<version> (supported runtimes: node; e.g. "node@25.5.0")."#
    )]
    #[diagnostic(code(ERR_PNPM_PACK_APP_INVALID_RUNTIME))]
    InvalidRuntime {
        #[error(not(source))]
        spec: String,
    },

    #[display(
        r#"Invalid --output-name "{name}". The name must be a plain filename without path separators, Windows-reserved names (e.g. CON, NUL), characters like <>:"|?* or NUL, and must not end in a dot or space."#
    )]
    #[diagnostic(code(ERR_PNPM_PACK_APP_INVALID_OUTPUT_NAME))]
    InvalidOutputName {
        #[error(not(source))]
        name: String,
    },

    #[display("Unknown \"pnpm.app.{key}\" setting in package.json. Allowed keys: {allowed}.")]
    #[diagnostic(code(ERR_PNPM_PACK_APP_INVALID_CONFIG))]
    UnknownConfigKey { key: String, allowed: String },

    #[diagnostic(code(ERR_PNPM_PACK_APP_INVALID_CONFIG))]
    InvalidConfig {
        #[error(not(source))]
        message: String,
    },

    #[display("Failed to parse {path}: {message}")]
    #[diagnostic(code(ERR_PNPM_PACK_APP_INVALID_PACKAGE_JSON))]
    InvalidPackageJson { path: String, message: String },

    #[display(r#"Could not determine the output name: package.json in {dir} has no "name" field."#)]
    #[diagnostic(
        code(ERR_PNPM_PACK_APP_NO_OUTPUT_NAME),
        help(r#"Pass --output-name <name> or set "pnpm.app.outputName" in package.json."#)
    )]
    NoOutputName {
        #[error(not(source))]
        dir: String,
    },

    #[display(
        "The embedded runtime \"node@{version}\" is older than Node.js v{major}.{minor}, which is the minimum version that supports --build-sea."
    )]
    #[diagnostic(
        code(ERR_PNPM_PACK_APP_RUNTIME_TOO_OLD),
        help(
            r#"Pass --runtime node@25.5.0 (or newer) or set "pnpm.app.runtime" in package.json."#
        )
    )]
    RuntimeTooOld {
        version: String,
        major: u64,
        minor: u64,
    },

    #[display(r#"Could not find a Node.js version that satisfies "{specifier}""#)]
    #[diagnostic(code(ERR_PNPM_PACK_APP_NODE_VERSION_NOT_FOUND))]
    NodeVersionNotFound {
        #[error(not(source))]
        specifier: String,
    },

    #[display(
        "Expected Node.js binary at {path} after installing node@runtime:{version}, but it was not found."
    )]
    #[diagnostic(code(ERR_PNPM_PACK_APP_NODE_BINARY_MISSING))]
    NodeBinaryMissing { path: String, version: String },

    #[display("Cross-compiled macOS binary at {path} could not be ad-hoc signed with \"ldid\".")]
    #[diagnostic(
        code(ERR_PNPM_PACK_APP_MACOS_SIGN_FAILED),
        help(
            r#"Install ldid (https://github.com/ProcursusTeam/ldid) or re-sign the binary on macOS with "codesign --sign - <file>"."#
        )
    )]
    MacosSignFailed {
        #[error(not(source))]
        path: String,
    },

    #[display("Cannot ad-hoc sign the macOS binary at {path} on a {host} host.")]
    #[diagnostic(
        code(ERR_PNPM_PACK_APP_MACOS_SIGN_UNSUPPORTED_HOST),
        help(
            r#"Build macOS targets on a macOS or Linux host, or re-sign the produced binary yourself with "codesign --sign -" on macOS."#
        )
    )]
    MacosSignUnsupportedHost { path: String, host: String },
}
