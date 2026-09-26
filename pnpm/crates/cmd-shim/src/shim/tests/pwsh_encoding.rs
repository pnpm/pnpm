use super::{ScriptRuntime, generate_pwsh_shim};
use std::{fs, path::Path, process::Command};
use tempfile::tempdir;

fn node_runtime() -> ScriptRuntime {
    ScriptRuntime { prog: Some("node".into()), args: String::new() }
}

#[test]
fn pwsh_shim_marks_unicode_text_as_utf8() {
    let shim = Path::new("/proj/.bin/cli.ps1");
    let unicode_target =
        generate_pwsh_shim(Path::new("/proj/工具/cli.js"), shim, Some(&node_runtime()), &[]);
    assert!(unicode_target.starts_with("\u{FEFF}#!/usr/bin/env pwsh\n"));

    let unicode_node_path = generate_pwsh_shim(
        Path::new("/proj/pkg/cli.js"),
        shim,
        Some(&node_runtime()),
        &["/工具/node_modules".to_string()],
    );
    assert!(unicode_node_path.starts_with("\u{FEFF}#!/usr/bin/env pwsh\n"));
}

#[test]
fn pwsh_shim_leaves_ascii_text_unmarked() {
    let body = generate_pwsh_shim(
        Path::new("/proj/pkg/cli.js"),
        Path::new("/proj/.bin/cli.ps1"),
        Some(&node_runtime()),
        &["/proj/node_modules".to_string()],
    );
    assert!(body.starts_with("#!/usr/bin/env pwsh\n"));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires Windows PowerShell 5.1")]
fn pwsh_shim_runs_unicode_target_under_windows_powershell() {
    let root = tempdir().unwrap();
    let target_dir = root.path().join("工具");
    fs::create_dir(&target_dir).unwrap();
    let target = target_dir.join("cli.js");
    fs::write(
        &target,
        "console.log(JSON.stringify(process.argv.slice(2)))\nprocess.exit(Number(process.argv[3]))\n",
    )
    .unwrap();
    let shim = root.path().join("shim.ps1");
    fs::write(&shim, generate_pwsh_shim(&target, &shim, Some(&node_runtime()), &[])).unwrap();

    for exit_code in [0, 7] {
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&shim)
            .args(["argument with spaces", &exit_code.to_string()])
            .output()
            .unwrap();
        eprintln!("exit {exit_code}: {output:?}");
        assert_eq!(output.status.code(), Some(exit_code));
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim_end(),
            format!(r#"["argument with spaces","{exit_code}"]"#),
        );
    }
}
