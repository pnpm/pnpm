//! Unit tests for the pure pieces of the Windows registry path-extender:
//! the `reg query` line parser, the code-page number extraction, and the
//! before/after report rendering. The registry-mutating commands need a real
//! Windows host and are exercised by `pacquet setup` end to end there.

use super::{
    AddDirToEnvPathOpts, AddingPosition, EnvVariableChange, PathExtenderError,
    add_dir_to_windows_env_path_inner, first_number, get_env_value_from_registry,
    run_capture_chcp_with,
};
use pretty_assertions::assert_eq;
use std::path::Path;

const SAMPLE: &str = "\r\nHKEY_CURRENT_USER\\Environment\r\n    Path    REG_EXPAND_SZ    C:\\Users\\me\\bin\r\n    PNPM_HOME    REG_SZ    C:\\pnpm\r\n";

#[test]
fn reads_a_value_by_name() {
    assert_eq!(get_env_value_from_registry(SAMPLE, "Path").as_deref(), Some(r"C:\Users\me\bin"));
    assert_eq!(get_env_value_from_registry(SAMPLE, "PNPM_HOME").as_deref(), Some(r"C:\pnpm"));
}

#[test]
fn matches_the_name_case_insensitively() {
    assert_eq!(get_env_value_from_registry(SAMPLE, "path").as_deref(), Some(r"C:\Users\me\bin"));
}

#[test]
fn missing_value_returns_none() {
    assert!(get_env_value_from_registry(SAMPLE, "NOT_THERE").is_none());
}

#[test]
fn first_number_extracts_the_code_page() {
    assert_eq!(first_number("Active code page: 437"), Some(437));
    assert_eq!(first_number("no digits"), None);
}

#[test]
fn rejects_pnpm_home_that_would_split_the_path() {
    // Validation runs before any `reg` command, so this is exercisable off
    // Windows. A `;` in PNPM_HOME would split the persisted Path.
    let opts = AddDirToEnvPathOpts {
        config_section_name: "pnpm",
        proxy_var_name: Some("PNPM_HOME"),
        proxy_var_sub_dir: Some("bin"),
        overwrite: false,
        position: AddingPosition::Start,
    };
    let err = add_dir_to_windows_env_path_inner(Path::new(r"C:\pnpm;C:\evil"), &opts)
        .expect_err("a semicolon in PNPM_HOME must be rejected");
    assert!(matches!(err, PathExtenderError::UnsafePnpmHome { character: ';', .. }));
}

#[test]
fn render_report_lists_changed_variables() {
    let changes = vec![
        EnvVariableChange {
            variable: "PNPM_HOME".to_string(),
            old_value: None,
            new_value: r"C:\pnpm".to_string(),
        },
        EnvVariableChange {
            variable: "Path".to_string(),
            old_value: Some(r"C:\old".to_string()),
            new_value: r"%PNPM_HOME%;C:\old".to_string(),
        },
    ];
    let report = super::super::render_windows_report(&changes);
    assert!(report.config_file.is_none());
    assert_eq!(report.old_settings, r"Path=C:\old");
    assert_eq!(report.new_settings, "PNPM_HOME=C:\\pnpm\nPath=%PNPM_HOME%;C:\\old");
}

#[test]
fn chcp_prefers_chcp_com_when_available() {
    let mut calls = Vec::new();
    let res = run_capture_chcp_with(&["65001"], |prog, args| {
        calls.push((prog.to_string(), args.iter().map(ToString::to_string).collect::<Vec<_>>()));
        Ok("Active code page: 65001\n".to_string())
    });
    assert!(res.is_ok());
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "chcp.com");
    assert_eq!(calls[0].1, vec!["65001"]);
}

#[test]
fn chcp_falls_back_to_chcp_when_chcp_com_not_found() {
    let mut calls = Vec::new();
    let res = run_capture_chcp_with(&["65001"], |prog, args| {
        calls.push((prog.to_string(), args.iter().map(ToString::to_string).collect::<Vec<_>>()));
        if prog == "chcp.com" {
            Err(PathExtenderError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "chcp.com not found",
            )))
        } else {
            Ok("Active code page: 65001\n".to_string())
        }
    });
    assert!(res.is_ok());
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "chcp.com");
    assert_eq!(calls[1].0, "chcp");
}

#[test]
fn chcp_propagates_non_not_found_errors_without_fallback() {
    let mut calls = Vec::new();
    let res = run_capture_chcp_with(&["65001"], |prog, args| {
        calls.push((prog.to_string(), args.iter().map(ToString::to_string).collect::<Vec<_>>()));
        Err(PathExtenderError::CommandFailed {
            command: prog.to_string(),
            stderr: "Access denied".to_string(),
        })
    });
    assert!(matches!(res, Err(PathExtenderError::CommandFailed { .. })));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "chcp.com");
}
