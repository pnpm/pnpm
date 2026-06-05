use super::{ScriptShellError, SelectedShell, select_shell};
use pretty_assertions::assert_eq;
use std::{ffi::OsString, path::Path};

fn os(text: &str) -> OsString {
    OsString::from(text)
}

/// Default on POSIX: `sh -c`, no verbatim args.
#[test]
fn posix_default_is_sh_minus_c() {
    let shell = select_shell(None, false).expect("select_shell");
    assert_eq!(
        shell,
        SelectedShell {
            program: Path::new("sh").to_path_buf(),
            args: vec![os("-c")],
            windows_verbatim_args: false,
        },
    );
}

/// Default on Windows: `cmd /d /s /c` with verbatim args. When
/// `ComSpec` / `COMSPEC` is set the env var wins; otherwise the
/// literal string `cmd` is used. We do not assert the program path
/// here because the runner env may or may not have `ComSpec` set —
/// the args + verbatim flag are the load-bearing part.
#[test]
fn windows_default_uses_cmd_with_d_s_c_and_verbatim_args() {
    let shell = select_shell(None, true).expect("select_shell");
    assert_eq!(shell.args, vec![os("/d"), os("/s"), os("/c")]);
    assert!(shell.windows_verbatim_args, "verbatim must be set for cmd.exe");
    let program = shell.program.to_string_lossy().to_ascii_lowercase();
    assert!(
        program == "cmd" || program.ends_with("cmd.exe"),
        "expected cmd or *cmd.exe, got {program:?}",
    );
}

/// A custom `scriptShell` (e.g. `/usr/local/bin/bash`) wins on both
/// platforms, with `-c`. This is the path exercised by upstream's
/// `skipOnWindows('pnpm run with custom shell')` test from
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/exec/commands/test/index.ts#L478-L508>.
#[test]
fn custom_script_shell_wins_on_both_platforms() {
    let custom = Path::new("/usr/local/bin/bash");
    for is_windows in [false, true] {
        let shell = select_shell(Some(custom), is_windows).expect("select_shell");
        assert_eq!(
            shell,
            SelectedShell {
                program: custom.to_path_buf(),
                args: vec![os("-c")],
                windows_verbatim_args: false,
            },
            "is_windows={is_windows}",
        );
    }
}

/// `.cmd` / `.bat` `script_shell` on Windows surfaces as
/// `ERR_PNPM_INVALID_SCRIPT_SHELL_WINDOWS`. Mirrors the upstream
/// guard at
/// <https://github.com/pnpm/pnpm/blob/b4f8f47ac2/exec/lifecycle/src/runLifecycleHook.ts#L63-L71>
/// and ports the `onlyOnWindows('pnpm shows error if script-shell is .cmd')`
/// test from `pnpm/exec/commands/test/index.ts:509-542`.
#[test]
fn batch_file_script_shell_rejected_on_windows() {
    for ext in [".cmd", ".CMD", ".bat", ".BAT"] {
        let path = format!("C:\\tools\\shell-mock{ext}");
        let err = select_shell(Some(Path::new(&path)), true).expect_err("must reject");
        match err {
            ScriptShellError::BatchFileOnWindows { path: got } => {
                assert_eq!(got, path, "error path must echo input");
            }
        }
    }
}

/// Same `.cmd` path on POSIX is allowed — we only block on Windows.
/// Upstream's `isWindowsBatchFile` gates on `process.platform === 'win32'`,
/// so a Linux user pointing scriptShell at `something.cmd` is left
/// alone (it'd fail elsewhere, but that's not this guard's job).
#[test]
fn batch_file_script_shell_allowed_on_posix() {
    let path = Path::new("/tmp/weird.cmd");
    let shell = select_shell(Some(path), false).expect("must accept on POSIX");
    assert_eq!(shell.program, path);
}
