use super::{ScriptRuntime, generate_cmd_shim};
use std::{fs, process::Command};
use tempfile::tempdir;

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
    fs::write(&shim, generate_cmd_shim(&target, &shim, Some(&runtime), &[])).unwrap();

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
            let output = Command::new("cmd.exe")
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
                let shadowed_output = Command::new("cmd.exe")
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
