use super::{PnpmExecCommandError, escape_control_characters, prepend_to_path, validate};
use pretty_assertions::assert_eq;
use serde_json::json;
use std::{ffi::OsString, path::Path};

#[test]
fn validate_accepts_an_argv_array() {
    let command = validate(&json!(["my-tool", "which-pnpm"])).expect("valid argv array");
    assert_eq!(command, vec!["my-tool".to_string(), "which-pnpm".to_string()]);
}

#[test]
fn validate_rejects_non_arrays_empty_arrays_and_non_string_items() {
    for value in [
        json!("my-tool which-pnpm"),
        json!([]),
        json!(["my-tool", 42]),
        json!(["my-tool", ""]),
        json!(null),
        json!({ "command": "my-tool" }),
    ] {
        let error = validate(&value).expect_err("malformed value must be rejected");
        assert!(matches!(error, PnpmExecCommandError::Invalid), "{value}: {error}");
    }
}

#[test]
fn prepend_to_path_prepends_the_bin_dir() {
    let delimiter = if cfg!(windows) { ";" } else { ":" };
    let out = prepend_to_path(Path::new("/vended/bin"), Some(OsString::from("/usr/bin")));
    assert_eq!(out, OsString::from(format!("/vended/bin{delimiter}/usr/bin")));
}

#[test]
fn prepend_to_path_skips_a_dir_already_leading_path() {
    let delimiter = if cfg!(windows) { ";" } else { ":" };
    let already_leading = OsString::from(format!("/vended/bin{delimiter}/usr/bin"));
    let out = prepend_to_path(Path::new("/vended/bin"), Some(already_leading.clone()));
    assert_eq!(out, already_leading);
}

#[test]
fn escape_control_characters_renders_json_escapes() {
    assert_eq!(escape_control_characters("plain ascii ünïcode"), "plain ascii ünïcode");
    assert_eq!(escape_control_characters("a\nb\tc\rd\u{8}e\u{c}f"), "a\\nb\\tc\\rd\\be\\ff");
    assert_eq!(escape_control_characters("\u{1b}[31mred\u{1b}[0m"), "\\u001b[31mred\\u001b[0m");
}

#[test]
fn prepend_to_path_handles_a_missing_or_empty_path() {
    assert_eq!(prepend_to_path(Path::new("/vended/bin"), None), OsString::from("/vended/bin"));
    assert_eq!(
        prepend_to_path(Path::new("/vended/bin"), Some(OsString::new())),
        OsString::from("/vended/bin"),
    );
}
