//! Add a directory (and an optional proxy variable such as `PNPM_HOME`) to
//! the user's `PATH` persistently.
//!
//! Ports pnpm's
//! [`@pnpm/os.env.path-extender`](https://github.com/pnpm/pnpm/blob/1819226b51/packages/path-extender/src/path-extender.ts):
//! on Windows the user registry is edited; on every other platform the
//! current shell's rc file is edited. The Windows changes are rendered into
//! the same `old_settings` / `new_settings` shape the POSIX path reports.

mod posix;
mod windows;

use derive_more::{Display, Error};
use miette::Diagnostic;
use std::path::PathBuf;

/// Where the new directory is inserted into `PATH`. Mirrors pnpm's
/// `AddingPosition` (defaulting to `start`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AddingPosition {
    Start,
    // `setup` always inserts at the start, but `end` is part of the ported
    // path-extender contract (and exercised by the renderer tests), so the
    // renderers keep handling it to stay aligned with upstream pnpm.
    #[allow(dead_code, reason = "ported path-extender API; setup only uses Start")]
    End,
}

/// How the shell config file changed. Mirrors pnpm's `ConfigFileChangeType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConfigFileChangeType {
    Skipped,
    Appended,
    Modified,
    Created,
}

/// The config file that was touched and how. `None` on Windows, where the
/// registry — not a file — is edited. Mirrors pnpm's `ConfigReport`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ConfigReport {
    pub path: PathBuf,
    pub change_type: ConfigFileChangeType,
}

/// The before/after of a path-extension run. Mirrors pnpm's
/// `PathExtenderReport`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PathExtenderReport {
    pub config_file: Option<ConfigReport>,
    pub old_settings: String,
    pub new_settings: String,
}

/// Options for [`add_dir_to_env_path`]. Mirrors pnpm's
/// `AddDirToPosixEnvPathOpts` (the superset the public `addDirToEnvPath`
/// accepts).
pub(super) struct AddDirToEnvPathOpts<'a> {
    pub config_section_name: &'a str,
    pub proxy_var_name: Option<&'a str>,
    pub proxy_var_sub_dir: Option<&'a str>,
    pub overwrite: bool,
    pub position: AddingPosition,
}

/// Errors raised while extending `PATH`. Codes mirror pnpm's `PnpmError`
/// codes (pnpm prefixes them with `ERR_PNPM_`).
#[derive(Debug, Display, Error, Diagnostic)]
pub(crate) enum PathExtenderError {
    #[display(
        r#"The config file at "{}" already contains a {config_section_name} section but with other configuration"#,
        config_file.display(),
    )]
    #[diagnostic(
        code(ERR_PNPM_BAD_SHELL_SECTION),
        help("If you want to override the existing configuration section, use the --force option")
    )]
    BadShellSection { config_file: PathBuf, config_section_name: String },

    #[display("Could not infer shell type.")]
    #[diagnostic(
        code(ERR_PNPM_UNKNOWN_SHELL),
        help(
            "Set the SHELL environment variable to your active shell.\nSupported shell languages are bash, zsh, fish, ksh, dash, sh, and nushell."
        )
    )]
    UnknownShell,

    #[display(r#"Can't setup configuration for "{shell}" shell"#)]
    #[diagnostic(
        code(ERR_PNPM_UNSUPPORTED_SHELL),
        help("Supported shell languages are bash, zsh, fish, ksh, dash, sh, and nushell.")
    )]
    UnsupportedShell { shell: String },

    #[display("Cannot find a config file for {shell}. The ENV environment variable is not set.")]
    #[diagnostic(code(ERR_PNPM_NO_SHELL_CONFIG))]
    NoShellConfig { shell: String },

    #[display("Could not determine the home directory")]
    NoHomeDir,

    #[display(r#"Invalid proxyVarSubDir: "{sub_dir}""#)]
    #[diagnostic(code(ERR_PNPM_INVALID_SUBDIR))]
    InvalidSubDir { sub_dir: String },

    // Hardening beyond pnpm's `@pnpm/os.env.path-extender`: a path-separator
    // (`:` on POSIX, `;` on Windows), a `%` (Windows `%PNPM_HOME%`
    // expansion), or a newline in `PNPM_HOME` would split the persisted
    // `PATH` into extra entries, so it is rejected rather than written.
    #[display(
        r#"The pnpm home directory "{dir}" contains a character ({character:?}) that is unsafe for the PATH"#
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_PNPM_HOME))]
    UnsafePnpmHome { dir: String, character: char },

    #[display("Currently '{env_name}' is set to '{wanted_value}'")]
    #[diagnostic(
        code(ERR_PNPM_BAD_ENV_FOUND),
        help("If you want to override the existing env variable, use the --force option")
    )]
    BadEnvFound { env_name: String, wanted_value: String },

    #[display("exec chcp failed: {message}")]
    #[diagnostic(code(ERR_PNPM_CHCP))]
    Chcp { message: String },

    #[display("`{command}` failed: {stderr}")]
    #[diagnostic(code(ERR_PNPM_SETUP_COMMAND_FAILED))]
    CommandFailed { command: String, stderr: String },

    #[display("win32 registry environment values could not be retrieved")]
    #[diagnostic(code(ERR_PNPM_REG_READ))]
    RegRead,

    #[display(r#""Path" environment variable is not found in the registry"#)]
    #[diagnostic(code(ERR_PNPM_NO_PATH))]
    NoPath,

    #[display(r#"Failed to set "{env_name}" to "{value}": {stderr}"#)]
    #[diagnostic(code(ERR_PNPM_FAILED_SET_ENV))]
    FailedSetEnv { env_name: String, value: String, stderr: String },

    #[display("{_0}")]
    Io(std::io::Error),

    #[display("{_0}")]
    EnsureFile(pacquet_fs::EnsureFileError),
}

impl From<std::io::Error> for PathExtenderError {
    fn from(err: std::io::Error) -> Self {
        PathExtenderError::Io(err)
    }
}

impl From<pacquet_fs::EnsureFileError> for PathExtenderError {
    fn from(err: pacquet_fs::EnsureFileError) -> Self {
        PathExtenderError::EnsureFile(err)
    }
}

/// Persistently add `dir` to the user's `PATH`. The proxy-variable
/// indirection (`PNPM_HOME` → `$PNPM_HOME/bin`) keeps the `PATH` entry
/// stable when the home directory moves. Mirrors pnpm's `addDirToEnvPath`.
pub(super) fn add_dir_to_env_path(
    dir: &std::path::Path,
    opts: &AddDirToEnvPathOpts,
) -> Result<PathExtenderReport, PathExtenderError> {
    if let Some(sub_dir) = opts.proxy_var_sub_dir
        && (sub_dir.starts_with('/')
            || sub_dir.starts_with('\\')
            || sub_dir.contains("..")
            || sub_dir.contains([';', '%', '"', '\'', '`', '$', '<', '>', '&', '|', '\n', '\r']))
    {
        return Err(PathExtenderError::InvalidSubDir { sub_dir: sub_dir.to_string() });
    }
    // Per-target compilation: the Windows registry path and the POSIX
    // rc-file path are both compiled everywhere, and `cfg!(windows)` selects
    // the right one at runtime, mirroring pnpm's `process.platform` branch.
    if cfg!(windows) {
        let changes = windows::add_dir_to_windows_env_path(dir, opts)?;
        Ok(render_windows_report(&changes))
    } else {
        posix::add_dir_to_posix_env_path(dir, opts)
    }
}

/// Render the per-variable Windows registry changes into the file-oriented
/// report shape. Mirrors pnpm's `renderWindowsReport`.
fn render_windows_report(changes: &[windows::EnvVariableChange]) -> PathExtenderReport {
    let mut old_settings = Vec::new();
    let mut new_settings = Vec::new();
    for change in changes {
        if let Some(old_value) = &change.old_value
            && !old_value.is_empty()
        {
            old_settings.push(format!("{}={}", change.variable, old_value));
        }
        if !change.new_value.is_empty() {
            new_settings.push(format!("{}={}", change.variable, change.new_value));
        }
    }
    PathExtenderReport {
        config_file: None,
        old_settings: old_settings.join("\n"),
        new_settings: new_settings.join("\n"),
    }
}
