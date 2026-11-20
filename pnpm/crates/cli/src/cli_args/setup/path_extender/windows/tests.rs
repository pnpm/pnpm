//! Unit tests for the pure pieces of the Windows registry path-extender:
//! the `reg query` line parser, the code-page number extraction, and the
//! before/after report rendering. The registry-mutating commands need a real
//! Windows host and are exercised by `pacquet setup` end to end there.

use super::{
    AddDirToEnvPathOpts,
    AddingPosition,
    EnvVariableChange,
    PathExtenderError,
    add_dir_to_windows_env_path_inner,
    first_number,
    get_env_value_from_registry,
    persisted_path_from_registry,
    run_capture_mode_with,
    split_registry_path,
    update_env_variable_with,
};
use pretty_assertions::assert_eq;
use std::path::{
    Path,
    PathBuf,
};

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

/// An unrelated variable whose name holds a multi-byte character must not
/// stop the search, even when its byte length exceeds the name we look for.
#[test]
fn skips_an_unrelated_name_holding_a_multibyte_character() {
    let output = "    ABCä    REG_SZ    ignored\r\n    Path    REG_EXPAND_SZ    C:\\tools\r\n";
    assert_eq!(get_env_value_from_registry(output, "Path").as_deref(), Some(r"C:\tools"));

    let output = "    ABCDEFGHä    REG_SZ    ignored\r\n";
    assert!(get_env_value_from_registry(output, "PNPM_HOME").is_none());
}

#[test]
fn rewrites_a_matching_value_when_the_registry_type_is_wrong() {
    let registry_output =
        "    PNPM_HOME    REG_EXPAND_SZ    C:\\pnpm\r\n    Path    REG_EXPAND_SZ    C:\\old\r\n";
    let mut writes = Vec::new();

    let change = update_env_variable_with(
        registry_output,
        "PNPM_HOME",
        r"C:\pnpm",
        false,
        false,
        |name, value, expandable_string| {
            writes.push((name.to_string(), value.to_string(), expandable_string));
            Ok(())
        },
    )
    .expect("matching value with wrong registry type should be repaired");

    assert_eq!(change.old_value.as_deref(), Some(r"C:\pnpm"));
    assert_eq!(change.new_value, r"C:\pnpm");
    assert_eq!(writes, vec![(String::from("PNPM_HOME"), String::from(r"C:\pnpm"), false)]);
}

#[test]
fn skips_a_matching_value_when_the_registry_type_is_correct() {
    let registry_output =
        "    PNPM_HOME    REG_SZ    C:\\pnpm\r\n    Path    REG_EXPAND_SZ    C:\\old\r\n";
    let mut writes = Vec::new();

    let change = update_env_variable_with(
        registry_output,
        "PNPM_HOME",
        r"C:\pnpm",
        false,
        false,
        |name, value, expandable_string| {
            writes.push((name.to_string(), value.to_string(), expandable_string));
            Ok(())
        },
    )
    .expect("matching value with the correct registry type should be left unchanged");

    assert_eq!(change.old_value.as_deref(), Some(r"C:\pnpm"));
    assert_eq!(change.new_value, r"C:\pnpm");
    assert!(writes.is_empty());
}

#[test]
fn refuses_to_replace_a_different_value_and_names_the_current_one() {
    let registry_output =
        "    PNPM_HOME    REG_SZ    C:\\old-pnpm\r\n    Path    REG_EXPAND_SZ    C:\\old\r\n";
    let mut writes = Vec::new();

    let err = update_env_variable_with(
        registry_output,
        "PNPM_HOME",
        r"C:\pnpm",
        false,
        false,
        |name, value, expandable_string| {
            writes.push((name.to_string(), value.to_string(), expandable_string));
            Ok(())
        },
    )
    .expect_err("a different value should not be overwritten without --force");

    assert_eq!(err.to_string(), r"Currently 'PNPM_HOME' is set to 'C:\old-pnpm'");
    assert!(writes.is_empty());
}

#[test]
fn first_number_extracts_the_code_page() {
    let mode_con_cp =
        "\r\nStatus for device CON:\r\n----------------------\r\n    Code page:      437\r\n\r\n";
    assert_eq!(first_number(mode_con_cp), Some(437));
    assert_eq!(first_number("Active code page: 65001"), Some(65001));
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
fn mode_prefers_mode_com_when_available() {
    let mut calls = Vec::new();
    let res = run_capture_mode_with(&["con", "cp", "select=65001"], |prog, args| {
        calls.push((
            prog.to_string(),
            args.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        ));
        Ok("Status for device CON:\n".to_string())
    });
    assert!(res.is_ok());
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "mode.com");
    assert_eq!(calls[0].1, vec!["con", "cp", "select=65001"]);
}

#[test]
fn mode_falls_back_to_mode_when_mode_com_not_found() {
    let mut calls = Vec::new();
    let res = run_capture_mode_with(&["con", "cp"], |prog, args| {
        calls.push((
            prog.to_string(),
            args.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        ));
        if prog == "mode.com" {
            Err(PathExtenderError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "mode.com not found",
            )))
        } else {
            Ok("Status for device CON:\n".to_string())
        }
    });
    assert!(res.is_ok());
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "mode.com");
    assert_eq!(calls[1].0, "mode");
    assert_eq!(calls[1].1, vec!["con", "cp"]);
}

#[test]
fn mode_propagates_non_not_found_errors_without_fallback() {
    let mut calls = Vec::new();
    let res = run_capture_mode_with(&["con", "cp", "select=99999"], |prog, args| {
        calls.push((
            prog.to_string(),
            args.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        ));
        Err(PathExtenderError::CommandFailed {
            command: prog.to_string(),
            message: "The code page specified is not valid.".to_string(),
        })
    });
    assert!(matches!(res, Err(PathExtenderError::CommandFailed { .. })));
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "mode.com");
}

#[test]
fn persisted_path_expands_only_expandable_values() {
    let machine = "\r\nHKEY_LOCAL_MACHINE\\...\\Environment\r\n    \
                   Path    REG_EXPAND_SZ    %SystemRoot%\\system32;;%Missing%\\bin;C:\\50%\r\n";
    let user = "\r\nHKEY_CURRENT_USER\\Environment\r\n    \
                Path    REG_SZ    %SystemRoot%\\tools;C:\\npm\r\n";
    let process_env = |name: &str| (name == "SystemRoot").then(|| r"C:\Windows".to_string());

    let dirs = persisted_path_from_registry(machine, user, process_env).unwrap();
    assert_eq!(
        dirs,
        [
            PathBuf::from(r"C:\Windows\system32"),
            PathBuf::from(r"%Missing%\bin"),
            PathBuf::from(r"C:\50%"),
            PathBuf::from(r"%SystemRoot%\tools"),
            PathBuf::from(r"C:\npm"),
        ],
    );
}

#[test]
fn split_registry_path_drops_quotes_and_keeps_quoted_separators() {
    let dirs = split_registry_path(r#""C:\npm";C:\pnpm-home\bin;"C:\a;b""#);
    assert_eq!(
        dirs,
        [PathBuf::from(r"C:\npm"), PathBuf::from(r"C:\pnpm-home\bin"), PathBuf::from(r"C:\a;b")],
    );
}

#[test]
fn persisted_path_expands_each_scope_with_its_own_variables() {
    let machine = "\r\nHKEY_LOCAL_MACHINE\\...\\Environment\r\n    \
                   Path    REG_EXPAND_SZ    %TOOLS%;C:\\Windows\r\n    \
                   TOOLS    REG_EXPAND_SZ    %SystemDrive%\\npm\r\n";
    let user = "\r\nHKEY_CURRENT_USER\\Environment\r\n    \
                Path    REG_EXPAND_SZ    %TOOLS%;%PNPM_HOME%\\bin\r\n    \
                PNPM_HOME    REG_SZ    C:\\pnpm-home\r\n    \
                TOOLS    REG_SZ    C:\\user-tools\r\n";
    let process_env = |name: &str| (name == "SystemDrive").then(|| "C:".to_string());

    let dirs = persisted_path_from_registry(machine, user, process_env).unwrap();
    assert_eq!(
        dirs,
        [
            PathBuf::from(r"C:\npm"),
            PathBuf::from(r"C:\Windows"),
            PathBuf::from(r"C:\user-tools"),
            PathBuf::from(r"C:\pnpm-home\bin"),
        ],
    );
}

#[test]
fn a_user_path_reference_to_path_is_the_machine_path() {
    let machine = "\r\nHKEY_LOCAL_MACHINE\\...\\Environment\r\n    \
                   Path    REG_EXPAND_SZ    C:\\Windows\r\n";
    let user = "\r\nHKEY_CURRENT_USER\\Environment\r\n    \
                Path    REG_EXPAND_SZ    %Path%;C:\\pnpm-home\\bin\r\n";

    let dirs = persisted_path_from_registry(machine, user, |_: &str| None).unwrap();
    assert_eq!(
        dirs,
        [
            PathBuf::from(r"C:\Windows"),
            PathBuf::from(r"C:\Windows"),
            PathBuf::from(r"C:\pnpm-home\bin"),
        ],
    );
}
