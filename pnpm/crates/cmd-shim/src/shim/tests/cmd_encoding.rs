use super::{CmdShimBatch, ScriptRuntime, generate_cmd_shim};
use std::{fs, path::Path, process::Command};
use tempfile::tempdir;

#[test]
fn cmd_shim_switches_encoding_before_unicode_node_path() {
    let target = Path::new("/proj/pkg/cli");
    let shim = Path::new("/proj/.bin/cli.cmd");
    let node_path = ["/工具/50% off/node_modules".to_string()];
    let body = generate_cmd_shim(target, shim, None, &node_path, CmdShimBatch::Kept);
    let encoding_switch = body
        .find("@\"%SystemRoot%\\System32\\chcp.com\" 65001 >NUL\r\n")
        .expect("Unicode literals must be read using UTF-8");
    let node_path_assignment = body
        .find(r#"@SET "NODE_PATH=\工具\50%% off\node_modules""#)
        .expect("the Unicode NODE_PATH must retain percent escaping");
    assert!(encoding_switch < node_path_assignment);
    assert!(body.ends_with("@EXIT /B %_PNPM_EXIT_CODE%\r\n"));
    assert!(!body.replace("\r\n", "").contains('\n'));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires Windows CMD code pages")]
fn cmd_shim_runs_unicode_target_and_preserves_codepage_and_exit_status() {
    let root = tempdir().unwrap();
    let target_dir = root.path().join("工具");
    fs::create_dir(&target_dir).unwrap();
    let target = target_dir.join("cli.js");
    fs::write(
        &target,
        "console.log(JSON.stringify(process.argv.slice(2)))\nprocess.exit(Number(process.argv[4]))\n",
    )
    .unwrap();
    let shim = root.path().join("shim.cmd");
    let runtime = ScriptRuntime { prog: Some("node".into()), args: String::new() };
    fs::write(&shim, generate_cmd_shim(&target, &shim, Some(&runtime), &[], CmdShimBatch::Kept))
        .unwrap();

    for codepage in [437, 936, 65001] {
        for exit_code in [0, 7] {
            let driver = format!(
                "@echo off\r\n\
                 @for /f \"tokens=2 delims=:\" %%a in ('chcp') do @set \"original_codepage=%%a\"\r\n\
                 @chcp {codepage}>nul\r\n\
                 @call shim.cmd \"argument with spaces\" \"a&b\" {exit_code}\r\n\
                 @set \"shim_exit=%errorlevel%\"\r\n\
                 @chcp\r\n\
                 @chcp %original_codepage%>nul\r\n\
                 @exit /b %shim_exit%\r\n",
            );
            fs::write(root.path().join("run.cmd"), driver).unwrap();
            let output = cmd_in_own_console()
                .args(["/d", "/c", "run.cmd"])
                .current_dir(root.path())
                .output()
                .unwrap();
            eprintln!("CP {codepage}, exit {exit_code}: {output:?}");
            assert_eq!(output.status.code(), Some(exit_code));
            let stdout = String::from_utf8(output.stdout).unwrap();
            eprintln!("STDOUT:\n{stdout}");
            let mut lines = stdout.lines();
            assert_eq!(
                lines.next().unwrap(),
                format!(r#"["argument with spaces","a&b","{exit_code}"]"#),
            );
            assert_eq!(
                lines
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .last()
                    .unwrap(),
                codepage.to_string(),
            );
            if codepage == 936 {
                fs::write(
                    root.path().join("exit-only.cmd"),
                    format!("@shim.cmd \"argument with spaces\" \"a&b\" {exit_code}\r\n"),
                )
                .unwrap();
                let shadowed_output = cmd_in_own_console()
                    .args(["/d", "/c", "exit-only.cmd"])
                    .env("ERRORLEVEL", "99")
                    .current_dir(root.path())
                    .output()
                    .unwrap();
                eprintln!("inherited ERRORLEVEL override: {shadowed_output:?}");
                assert_eq!(shadowed_output.status.code(), Some(exit_code));
            }
        }
    }
}

/// A shim that ends its batch context runs nothing after its last line, so
/// that line restores the caller's code page itself, before the target runs.
#[test]
fn batchless_cmd_shim_restores_the_codepage_before_the_target() {
    let target = Path::new("/home/工具/pnpm.exe");
    let shim = Path::new("/home/bin/pnpm.cmd");
    let body = generate_cmd_shim(target, shim, None, &[], CmdShimBatch::EndedBeforeTarget);
    let encoding_switch = body
        .find(r#"@"%SystemRoot%\System32\chcp.com" 65001 >NUL"#)
        .expect("Unicode literals must be read using UTF-8");
    let last_line = body
        .trim_end_matches("\r\n")
        .rsplit("\r\n")
        .next()
        .unwrap();
    assert!(encoding_switch < body.find(last_line).unwrap());
    assert!(
        last_line.ends_with(
            r#"& (IF NOT "%_PNPM_CODEPAGE%"=="" "%SystemRoot%\System32\chcp.com" %_PNPM_CODEPAGE% >NUL) & "%~dp0\..\工具\pnpm.exe"  %*"#,
        ),
        "last line: {last_line}",
    );
    assert!(!body.contains("EXIT /B"), "nothing after the last line runs, body:\n{body}");
}

#[test]
#[cfg_attr(not(windows), ignore = "requires Windows CMD code pages")]
fn batchless_cmd_shim_runs_unicode_target_and_preserves_codepage_and_exit_status() {
    let root = tempdir().unwrap();
    let target_dir = root.path().join("工具");
    fs::create_dir(&target_dir).unwrap();
    let node = node_executable();
    // The target runs directly, as the pnpm CLI does: a copy of `node.exe`, with
    // the script handed over as a shebang argument.
    let target = target_dir.join("node.exe");
    fs::copy(&node, &target).unwrap();
    let script = target_dir.join("cli.js");
    fs::write(
        &script,
        "console.log(JSON.stringify(process.argv.slice(2)))\n\
         console.log(require('child_process').execSync('chcp', { encoding: 'latin1' }).trim())\n\
         process.exit(Number(process.argv[4]))\n",
    )
    .unwrap();
    let shim = root.path().join("shim.cmd");
    let runtime = ScriptRuntime { prog: None, args: format!(r#" "{}""#, script.display()) };
    let body =
        generate_cmd_shim(&target, &shim, Some(&runtime), &[], CmdShimBatch::EndedBeforeTarget);
    eprintln!("shim:\n{body}");
    fs::write(&shim, body).unwrap();

    for codepage in [437, 936, 65001] {
        for exit_code in [0, 7] {
            let driver = format!(
                "@echo off\r\n\
                 @for /f \"tokens=2 delims=:\" %%a in ('chcp') do @set \"original_codepage=%%a\"\r\n\
                 @chcp {codepage}>nul\r\n\
                 @call shim.cmd \"argument with spaces\" \"a&b\" {exit_code}\r\n\
                 @set \"shim_exit=%errorlevel%\"\r\n\
                 @chcp\r\n\
                 @chcp %original_codepage%>nul\r\n\
                 @exit /b %shim_exit%\r\n",
            );
            fs::write(root.path().join("run.cmd"), driver).unwrap();
            let output = cmd_in_own_console()
                .args(["/d", "/c", "run.cmd"])
                .current_dir(root.path())
                .output()
                .unwrap();
            eprintln!("CP {codepage}, exit {exit_code}: {output:?}");
            assert_eq!(output.status.code(), Some(exit_code));
            let stdout = String::from_utf8_lossy(&output.stdout);
            eprintln!("STDOUT:\n{stdout}");
            let mut lines = stdout.lines();
            assert_eq!(
                lines.next().unwrap(),
                format!(r#"["argument with spaces","a&b","{exit_code}"]"#),
            );
            // The target runs under the caller's code page, and the caller has
            // it back afterwards.
            for _ in 0..2 {
                assert_eq!(
                    lines
                        .next()
                        .unwrap()
                        .split_whitespace()
                        .last()
                        .unwrap(),
                    codepage.to_string(),
                );
            }
        }
    }
}

/// The interpreter itself, not whatever launcher answers to `node` on `PATH`:
/// a version manager's launcher may dispatch on its own file name.
fn node_executable() -> std::path::PathBuf {
    let output = Command::new("node")
        .args(["-p", "process.execPath"])
        .output()
        .unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .into()
}

/// `cmd.exe` in a console of its own. The code page belongs to the console, so
/// a test that switches it would otherwise race every other process attached to
/// the test runner's console.
pub(super) fn cmd_in_own_console() -> Command {
    let command = Command::new("cmd.exe");
    #[cfg(windows)]
    let command = {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut command = command;
        std::os::windows::process::CommandExt::creation_flags(&mut command, CREATE_NO_WINDOW);
        command
    };
    command
}
