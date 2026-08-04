use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
};

/// Failure to pick a shell for a lifecycle hook.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum ScriptShellError {
    /// Setting `scriptShell` to a `.bat` or `.cmd` file on Windows is
    /// blocked because Node refuses to spawn batch files without
    /// `shell: true`, and re-escaping arguments for a shell-wrapped
    /// batch invocation is unsafe (cf. CVE-2024-27980 / CVE-2024-24576).
    #[display(
        "Cannot spawn .bat or .cmd as a script shell. \
         The pnpm-workspace.yaml scriptShell option was configured to a .bat or .cmd file. \
         These cannot be used as a script shell reliably. \
         Please unset the scriptShell option, or configure it to a .exe instead. \
         (scriptShell={path})"
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_SCRIPT_SHELL_WINDOWS))]
    BatchFileOnWindows { path: String },
}

/// The result of [`select_shell`]: a program path plus the leading
/// flag arguments (`-c`, `/d /s /c`, etc.) that go before the script
/// body when spawning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedShell {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// Whether Node's `windowsVerbatimArguments` flag would have
    /// fired for this combination. Used by the Windows caller to opt
    /// into Rust's `std::os::windows::process::CommandExt::raw_arg`
    /// (Windows-only API; not linked as an intra-doc reference
    /// because rustdoc on non-Windows targets cannot resolve it).
    ///
    /// On non-Windows platforms the field is set but ignored —
    /// keeping the struct platform-independent simplifies tests.
    pub windows_verbatim_args: bool,
}

/// Pick the shell to spawn a lifecycle script under.
///
/// `is_windows` lets tests drive both branches without `#[cfg(windows)]`
/// gating the test bodies. Production callers pass `cfg!(windows)`.
pub fn select_shell(
    script_shell: Option<&Path>,
    is_windows: bool,
) -> Result<SelectedShell, ScriptShellError> {
    if is_windows
        && let Some(p) = script_shell
        && is_windows_batch_file(p)
    {
        return Err(ScriptShellError::BatchFileOnWindows {
            path: p.to_string_lossy().into_owned(),
        });
    }

    if let Some(p) = script_shell {
        return Ok(SelectedShell {
            program: p.to_path_buf(),
            args: vec![OsString::from("-c")],
            windows_verbatim_args: false,
        });
    }

    if is_windows {
        let comspec = env::var_os("ComSpec")
            .or_else(|| env::var_os("COMSPEC"))
            .map_or_else(|| PathBuf::from("cmd"), PathBuf::from);
        return Ok(SelectedShell {
            program: comspec,
            args: vec![OsString::from("/d"), OsString::from("/s"), OsString::from("/c")],
            windows_verbatim_args: true,
        });
    }

    Ok(SelectedShell {
        program: PathBuf::from("sh"),
        args: vec![OsString::from("-c")],
        windows_verbatim_args: false,
    })
}

/// `.cmd` / `.bat` suffix check, case-insensitive on the suffix. The
/// upstream `isWindowsBatchFile` also gates on `process.platform === 'win32'`;
/// here we factor that out and let the caller pass `is_windows`.
fn is_windows_batch_file(path: &Path) -> bool {
    let lowered = path.to_string_lossy().to_ascii_lowercase();
    lowered.ends_with(".cmd") || lowered.ends_with(".bat")
}

#[cfg(test)]
mod tests;
