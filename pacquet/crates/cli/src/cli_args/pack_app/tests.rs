//! Ports of pnpm's pack-app validation tests
//! (<https://github.com/pnpm/pnpm/blob/9f3df6b9b4/pnpm11/releasing/commands/src/pack-app/test/pack-app/index.test.ts>).
//!
//! These exercise the fail-fast validation paths that run before any
//! network or build work — exactly the branches the upstream Jest suite
//! covers. The download / SEA-injection paths spawn real subprocesses and
//! are out of scope for unit tests.

use std::fs;

use pacquet_config::Config;
use tempfile::TempDir;

use super::{
    PackAppArgs, is_reserved_windows_name, parse_runtime, parse_target, read_project_app_config,
    validate_output_name,
};

fn args() -> PackAppArgs {
    PackAppArgs {
        params: Vec::new(),
        entry: None,
        target: Vec::new(),
        runtime: None,
        output_dir: None,
        output_name: None,
    }
}

/// Run `PackAppArgs::run` against a temp dir and return the diagnostic
/// code of the error it surfaces.
fn run_and_get_code(dir: &TempDir, args: PackAppArgs) -> String {
    let config = Config::default();
    let report = pacquet_tokio_block_on(args.run(&config, dir.path()))
        .expect_err("pack-app should fail before any build work");
    diagnostic_code(report)
}

fn pacquet_tokio_block_on<Fut: std::future::Future>(future: Fut) -> Fut::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build a tokio runtime")
        .block_on(future)
}

fn diagnostic_code(report: impl Into<miette::Report>) -> String {
    report.into().code().map(|code| code.to_string()).unwrap_or_default()
}

#[test]
fn fails_fast_when_no_entry_is_provided() {
    let dir = TempDir::new().unwrap();
    assert_eq!(run_and_get_code(&dir, args()), "ERR_PNPM_PACK_APP_MISSING_ENTRY");
}

#[test]
fn fails_fast_when_the_entry_file_does_not_exist() {
    let dir = TempDir::new().unwrap();
    let code =
        run_and_get_code(&dir, PackAppArgs { entry: Some("missing.cjs".to_string()), ..args() });
    assert_eq!(code, "ERR_PNPM_PACK_APP_ENTRY_NOT_FOUND");
}

#[test]
fn fails_fast_when_the_entry_path_is_a_directory() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("entry-dir")).unwrap();
    let code =
        run_and_get_code(&dir, PackAppArgs { entry: Some("entry-dir".to_string()), ..args() });
    assert_eq!(code, "ERR_PNPM_PACK_APP_ENTRY_NOT_FILE");
}

#[test]
fn reads_entry_from_pnpm_app_entry_when_entry_is_omitted() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name":"test-app","pnpm":{"app":{"entry":"from-config.cjs"}}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("from-config.cjs"), "module.exports = {}").unwrap();
    // With entry from config but no target, we hit MISSING_TARGET — enough
    // to prove the entry was picked up from pnpm.app.entry.
    assert_eq!(run_and_get_code(&dir, args()), "ERR_PNPM_PACK_APP_MISSING_TARGET");
}

#[test]
fn reads_targets_from_pnpm_app_targets_when_target_is_omitted() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name":"test-app","pnpm":{"app":{"targets":["bad-target"]}}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("entry.cjs"), "module.exports = {}").unwrap();
    let code =
        run_and_get_code(&dir, PackAppArgs { entry: Some("entry.cjs".to_string()), ..args() });
    assert_eq!(code, "ERR_PNPM_PACK_APP_INVALID_TARGET");
}

#[test]
fn rejects_entry_that_escapes_the_project() {
    for entry in ["../outside.cjs", "../../etc/passwd", "/etc/passwd", "sub/../../escape.cjs"] {
        let dir = TempDir::new().unwrap();
        let code = run_and_get_code(&dir, PackAppArgs { entry: Some(entry.to_string()), ..args() });
        assert_eq!(code, "ERR_PNPM_PACK_APP_ENTRY_OUTSIDE_PROJECT", "entry: {entry}");
    }
}

#[test]
fn rejects_output_dir_that_escapes_the_project() {
    for output_dir in ["../pwn", "../../tmp/pwn", "/tmp/pwn", "sub/../../pwn"] {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("entry.cjs"), "module.exports = {}").unwrap();
        let code = run_and_get_code(
            &dir,
            PackAppArgs {
                entry: Some("entry.cjs".to_string()),
                target: vec!["linux-x64".to_string()],
                output_name: Some("app".to_string()),
                output_dir: Some(output_dir.to_string()),
                ..args()
            },
        );
        assert_eq!(
            code, "ERR_PNPM_PACK_APP_OUTPUT_DIR_OUTSIDE_PROJECT",
            "output_dir: {output_dir}",
        );
    }
}

#[cfg(unix)]
#[test]
fn rejects_entry_symlinked_outside_the_project() {
    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let secret = outside.path().join("secret.cjs");
    fs::write(&secret, "module.exports = {}").unwrap();
    std::os::unix::fs::symlink(&secret, dir.path().join("entry.cjs")).unwrap();
    let code =
        run_and_get_code(&dir, PackAppArgs { entry: Some("entry.cjs".to_string()), ..args() });
    assert_eq!(code, "ERR_PNPM_PACK_APP_ENTRY_OUTSIDE_PROJECT");
}

#[cfg(unix)]
#[test]
fn rejects_output_dir_symlinked_outside_the_project() {
    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    fs::write(dir.path().join("entry.cjs"), "module.exports = {}").unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("dist-app")).unwrap();
    let code = run_and_get_code(
        &dir,
        PackAppArgs {
            entry: Some("entry.cjs".to_string()),
            target: vec!["linux-x64".to_string()],
            output_name: Some("app".to_string()),
            // default output dir is `dist-app`, the symlink created above
            ..args()
        },
    );
    assert_eq!(code, "ERR_PNPM_PACK_APP_OUTPUT_DIR_OUTSIDE_PROJECT");
}

#[cfg(unix)]
#[test]
fn rejects_output_file_that_is_a_preexisting_symlink() {
    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let victim = outside.path().join("victim");
    fs::write(&victim, "do not overwrite").unwrap();
    fs::write(dir.path().join("entry.cjs"), "module.exports = {}").unwrap();
    // The committed output path `dist-app/linux-x64/app` is a symlink to a
    // file outside the project; `node --build-sea` must not write through it.
    let target_dir = dir.path().join("dist-app").join("linux-x64");
    fs::create_dir_all(&target_dir).unwrap();
    std::os::unix::fs::symlink(&victim, target_dir.join("app")).unwrap();
    let code = run_and_get_code(
        &dir,
        PackAppArgs {
            entry: Some("entry.cjs".to_string()),
            target: vec!["linux-x64".to_string()],
            output_name: Some("app".to_string()),
            ..args()
        },
    );
    assert_eq!(code, "ERR_PNPM_PACK_APP_OUTPUT_FILE_NOT_REGULAR");
}

#[test]
fn rejects_unknown_keys_in_pnpm_app() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"name":"test-app","pnpm":{"app":{"entry":"entry.cjs","bogus":"yes"}}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("entry.cjs"), "module.exports = {}").unwrap();
    assert_eq!(run_and_get_code(&dir, args()), "ERR_PNPM_PACK_APP_INVALID_CONFIG");
}

#[test]
fn rejects_malformed_pnpm_app() {
    let cases = [
        r#"{"name":"a","pnpm":{"app":{"entry":42}}}"#,
        r#"{"name":"a","pnpm":{"app":{"targets":"linux-x64"}}}"#,
        r#"{"name":"a","pnpm":{"app":{"targets":["linux-x64",7]}}}"#,
        r#"{"name":"a","pnpm":{"app":{"runtime":["node@25"]}}}"#,
    ];
    for manifest in cases {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("package.json"), manifest).unwrap();
        let code = diagnostic_code(read_project_app_config(dir.path()).unwrap_err());
        assert_eq!(code, "ERR_PNPM_PACK_APP_INVALID_CONFIG", "manifest: {manifest}");
    }
}

#[test]
fn rejects_invalid_targets() {
    let cases = [
        "freebsd-x64",
        "linux-mips",
        "linux-x64-gnu",
        "linux", // incomplete
        "linux-x64-musl-extra",
        "linux-x64-musl-../../pwn",
        "macos-arm64", // legacy macos alias
        "win-x64",     // legacy win alias
        "LINUX-x64",
        " linux-x64",
    ];
    for target in cases {
        let err = parse_target(target).unwrap_err();
        assert_eq!(diagnostic_code(err), "ERR_PNPM_PACK_APP_INVALID_TARGET", "target: {target}");
    }
}

#[test]
fn rejects_musl_on_non_linux() {
    let err = parse_target("darwin-arm64-musl").unwrap_err();
    assert_eq!(diagnostic_code(err), "ERR_PNPM_PACK_APP_INVALID_TARGET");
}

#[test]
fn accepts_valid_targets() {
    for target in ["linux-x64", "linux-x64-musl", "linux-arm64", "darwin-arm64", "win32-x64"] {
        assert!(parse_target(target).is_ok(), "target should be valid: {target}");
    }
}

#[test]
fn rejects_invalid_runtimes() {
    for runtime in ["22", "22.0.0", "bun@1.0.0", "node@", "@", "NODE@22"] {
        let err = parse_runtime(runtime).unwrap_err();
        assert_eq!(diagnostic_code(err), "ERR_PNPM_PACK_APP_INVALID_RUNTIME", "runtime: {runtime}");
    }
}

#[test]
fn accepts_valid_runtimes() {
    assert_eq!(parse_runtime("node@25.5.0").unwrap(), "25.5.0");
    assert_eq!(parse_runtime("node@25").unwrap(), "25");
}

#[test]
fn rejects_invalid_output_names() {
    let cases = [
        "sub/dir",
        r"sub\dir",
        "..",
        "../pwn",
        "/tmp/pwn",
        ".",
        "pwn\0",
        "",
        "CON",
        "nul.exe",
        "COM1",
        "my:tool",
        "my|tool",
        "my?tool",
        "my*tool",
        "my<tool",
        "my>tool",
        r#"my"tool"#,
        "tool.",
        "tool ",
    ];
    for name in cases {
        assert!(validate_output_name(name).is_err(), "output name should be rejected: {name:?}");
    }
}

#[test]
fn accepts_valid_output_names() {
    for name in ["mytool", "my-tool", "my_tool", "tool.exe", "com0", "lpt0"] {
        assert!(validate_output_name(name).is_ok(), "output name should be accepted: {name:?}");
    }
}

#[test]
fn reserved_windows_names_are_detected() {
    for name in ["CON", "con", "PRN", "AUX", "NUL", "COM1", "LPT9", "nul.exe", "com1.txt"] {
        assert!(is_reserved_windows_name(name), "should be reserved: {name}");
    }
    for name in ["com0", "lpt0", "common", "console", "com10"] {
        assert!(!is_reserved_windows_name(name), "should not be reserved: {name}");
    }
}

#[test]
fn output_name_falls_back_to_unscoped_package_name() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("package.json"), r#"{"name":"@scope/my-app"}"#).unwrap();
    let project = read_project_app_config(dir.path()).unwrap();
    let name = super::derive_output_name_from_package(&project, dir.path()).unwrap();
    assert_eq!(name, "my-app");
    assert!(validate_output_name(&name).is_ok());
}

#[test]
fn no_output_name_without_package_name() {
    let dir = TempDir::new().unwrap();
    let project = read_project_app_config(dir.path()).unwrap();
    let err = super::derive_output_name_from_package(&project, dir.path()).unwrap_err();
    assert_eq!(diagnostic_code(err), "ERR_PNPM_PACK_APP_NO_OUTPUT_NAME");
}
