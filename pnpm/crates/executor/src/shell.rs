use derive_more::{Display, Error};
use miette::Diagnostic;
use std::{
    borrow::Cow,
    env,
    ffi::OsString,
    io,
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

    #[display(
        "The configured scriptShell was not found: {path}. \
         Point scriptShell at an existing shell executable, or unset it to use the default shell."
    )]
    #[diagnostic(code(ERR_PNPM_SCRIPT_SHELL_NOT_FOUND))]
    NotFound {
        path: String,
        #[error(source)]
        source: io::Error,
    },
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
        if is_windows && is_cmd_exe(p) {
            return Ok(SelectedShell {
                program: p.to_path_buf(),
                args: cmd_exe_args(),
                windows_verbatim_args: true,
            });
        }
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
            args: cmd_exe_args(),
            windows_verbatim_args: true,
        });
    }

    Ok(SelectedShell {
        program: PathBuf::from("sh"),
        args: vec![OsString::from("-c")],
        windows_verbatim_args: false,
    })
}

/// The text `shell` executes for `command`.
///
/// A Bourne shell that stays the script's parent holds a terminal `SIGINT`
/// until the foreground command exits. Without a trap, dash and zsh die
/// from that signal even when the command handled it and exited with a
/// status, which pnpm would report as a lifecycle failure
/// (<https://github.com/pnpm/pnpm/issues/9945>). The trap makes every such
/// shell do what bash does: re-raise `SIGINT` when the command died from it
/// (status 130), and otherwise carry on with the rest of the script. A
/// second interrupt always re-raises, so `Ctrl+C` can still stop a loop of
/// builtins.
pub(crate) fn script_body<'a>(shell: &SelectedShell, command: &'a str) -> Cow<'a, str> {
    if !returns_interrupted_child_status(shell) {
        return Cow::Borrowed(command);
    }
    Cow::Owned(format!("{INTERRUPT_STATUS_TRAP}{command}"))
}

/// `sh -c` prefix. `$?` must be read first: it is the interrupted command's
/// status only until the trap runs a command of its own. The first handled
/// interrupt replaces the trap with one that always re-raises, which keeps
/// the state out of any shell variable a script could set.
const INTERRUPT_STATUS_TRAP: &str = "\
trap 'if [ \"$?\" -eq 130 ]; then trap - INT; kill -s INT $$; fi; trap \"trap - INT; kill -s INT $$\" INT' INT; ";

fn returns_interrupted_child_status(shell: &SelectedShell) -> bool {
    if shell.windows_verbatim_args {
        return false;
    }
    let Some(flag) = shell.args.first().and_then(|arg| arg.to_str()) else {
        return false;
    };
    if flag != "-c" {
        return false;
    }
    matches!(
        shell_program_name(&shell.program).as_str(),
        "sh" | "dash" | "bash" | "ash" | "zsh" | "ksh" | "mksh",
    )
}

fn shell_program_name(program: &Path) -> String {
    let mut name = program
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name.ends_with(".exe") {
        name.truncate(name.len() - 4);
    }
    name
}

fn cmd_exe_args() -> Vec<OsString> {
    vec![OsString::from("/d"), OsString::from("/s"), OsString::from("/c")]
}

/// `cmd.exe` does not understand `-c` and takes the first `/c` anywhere on
/// its command line as its switch, so a script such as
/// `node install/can-compile` must follow `/d /s /c`. Splits on both
/// separators so the check does not depend on the host's `Path` parser.
fn is_cmd_exe(path: &Path) -> bool {
    let lossy = path.to_string_lossy();
    let basename = lossy
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    basename == "cmd" || basename == "cmd.exe"
}

/// `.cmd` / `.bat` suffix check, case-insensitive on the suffix. The
/// upstream `isWindowsBatchFile` also gates on `process.platform === 'win32'`;
/// here we factor that out and let the caller pass `is_windows`.
fn is_windows_batch_file(path: &Path) -> bool {
    let lowered = path.to_string_lossy().to_ascii_lowercase();
    lowered.ends_with(".cmd") || lowered.ends_with(".bat")
}

/// Blame a failed spawn on the configured `scriptShell` when the program
/// was not found. A missing working directory fails the spawn with the
/// same error kind, so the shell is blamed only when `cwd` exists.
pub(crate) fn missing_script_shell(
    script_shell: Option<&Path>,
    error: io::Error,
    cwd: &Path,
) -> Result<ScriptShellError, io::Error> {
    match script_shell {
        Some(path) if error.kind() == io::ErrorKind::NotFound && cwd.is_dir() => {
            Ok(ScriptShellError::NotFound {
                path: path.to_string_lossy().into_owned(),
                source: error,
            })
        }
        _ => Err(error),
    }
}

#[cfg(test)]
mod tests;
