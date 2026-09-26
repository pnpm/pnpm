use super::{
    ScriptShellError, SelectedShell, missing_script_shell, select_shell, use_shell_emulator,
};
use pretty_assertions::assert_eq;
use std::{ffi::OsString, io, path::Path};

fn os(text: &str) -> OsString {
    OsString::from(text)
}

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

/// We do not assert the program path here because the runner env may
/// or may not have `ComSpec` set — the args + verbatim flag are the
/// load-bearing part.
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

#[test]
fn shell_emulator_applies_only_when_script_shell_is_unset() {
    assert!(use_shell_emulator(true, None));
    assert!(!use_shell_emulator(false, None));
    let bash = Path::new(r"C:\Program Files\Git\bin\bash.exe");
    assert!(!use_shell_emulator(true, Some(bash)));
    assert!(!use_shell_emulator(false, Some(bash)));
}

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

#[test]
fn spawn_not_found_is_blamed_on_the_configured_script_shell() {
    let cwd = tempfile::tempdir().expect("tempdir");
    let shell = Path::new("/usr/bin/no-such-shell");
    let error = io::Error::from(io::ErrorKind::NotFound);
    match missing_script_shell(Some(shell), error, cwd.path()) {
        Ok(ScriptShellError::NotFound { path, .. }) => assert_eq!(path, shell.to_string_lossy()),
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn spawn_not_found_is_not_blamed_on_the_shell_when_the_cwd_is_missing() {
    let cwd = tempfile::tempdir().expect("tempdir");
    let missing_cwd = cwd.path().join("missing");
    let error = io::Error::from(io::ErrorKind::NotFound);
    let result = missing_script_shell(Some(Path::new("bash")), error, &missing_cwd);
    assert!(result.is_err(), "got {result:?}");
}

#[test]
fn spawn_not_found_without_a_configured_script_shell_is_kept() {
    let cwd = tempfile::tempdir().expect("tempdir");
    let error = io::Error::from(io::ErrorKind::NotFound);
    let result = missing_script_shell(None, error, cwd.path());
    assert!(result.is_err(), "got {result:?}");
}

#[test]
fn batch_file_script_shell_rejected_on_windows() {
    for ext in [".cmd", ".CMD", ".bat", ".BAT"] {
        let path = format!(r"C:\tools\shell-mock{ext}");
        let err = select_shell(Some(Path::new(&path)), true).expect_err("must reject");
        match err {
            ScriptShellError::BatchFileOnWindows { path: got } => {
                assert_eq!(got, path, "error path must echo input");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}

/// Upstream's `isWindowsBatchFile` gates on `process.platform === 'win32'`,
/// so a Linux user pointing scriptShell at `something.cmd` is left
/// alone (it'd fail elsewhere, but that's not this guard's job).
#[test]
fn batch_file_script_shell_allowed_on_posix() {
    let path = Path::new("/tmp/weird.cmd");
    let shell = select_shell(Some(path), false).expect("must accept on POSIX");
    assert_eq!(shell.program, path);
}

#[test]
fn cmd_exe_script_shell_uses_d_s_c_and_verbatim_args_on_windows() {
    for path in
        [r"C:\Windows\System32\cmd.exe", "C:/Windows/System32/CMD.EXE", "cmd.exe", "cmd", "Cmd"]
    {
        let shell = select_shell(Some(Path::new(path)), true).expect("select_shell");
        assert_eq!(
            shell,
            SelectedShell {
                program: Path::new(path).to_path_buf(),
                args: vec![os("/d"), os("/s"), os("/c")],
                windows_verbatim_args: true,
            },
            "scriptShell={path}",
        );
    }
}

#[test]
fn script_shell_named_cmd_keeps_minus_c_on_posix() {
    let path = Path::new("/usr/local/bin/cmd");
    let shell = select_shell(Some(path), false).expect("select_shell");
    assert_eq!(shell.args, vec![os("-c")]);
    assert!(!shell.windows_verbatim_args);
}
