use super::{ArgQuoting, build_command, posix_quote, quote_for_cmd};
use std::{ffi::OsStr, fs};
use tempfile::tempdir;

const CMD: ArgQuoting = ArgQuoting::Cmd { double_escape: false };

#[test]
fn posix_quote_leaves_safe_strings_unquoted() {
    assert_eq!(posix_quote("hello-world"), "hello-world");
    assert_eq!(posix_quote("a_b@1.0.0/path:to,thing"), "a_b@1.0.0/path:to,thing");
}

#[test]
fn posix_quote_wraps_unsafe_strings() {
    assert_eq!(posix_quote(""), "''");
    assert_eq!(posix_quote("a b"), "'a b'");
    assert_eq!(posix_quote("two words"), "'two words'");
}

#[test]
fn posix_quote_escapes_embedded_single_quotes() {
    assert_eq!(posix_quote("it's"), r#"'it'"'"'s'"#);
}

#[test]
fn build_command_without_args_returns_script_unchanged() {
    for quoting in [ArgQuoting::Posix, CMD] {
        assert_eq!(build_command("tsc --build", &[], quoting), "tsc --build");
    }
}

#[test]
fn build_command_appends_posix_quoted_args() {
    let args = ["plain".to_string(), "needs quoting".to_string()];
    assert_eq!(build_command("echo", &args, ArgQuoting::Posix), "echo plain 'needs quoting'");
}

#[test]
fn quote_for_cmd_matches_npm() {
    let cases = [
        ("plain", "plain"),
        ("", r#""""#),
        (r"C:\Program Files\tool", r#"^"C:\Program^ Files\tool^""#),
        (r"C:\Program Files\tool\", r#"^"C:\Program^ Files\tool\\^""#),
        (r"C:\dir\", r"C:\dir\"),
        ("%PATH%", "^%PATH^%"),
        (r#"a"b"#, r#"^"a\^"b^""#),
        (r#"a\"b"#, r#"^"a\\\^"b^""#),
        ("tab\there", "^\"tab\there^\""),
        ("^&|<>()!", "^^^&^|^<^>^(^)^!"),
        ("line\nbreak", r"line\nbreak"),
    ];
    for (arg, expected) in cases {
        assert_eq!(quote_for_cmd(arg, false), expected, "quoting {arg:?}");
    }
}

#[test]
fn quote_for_cmd_escapes_twice_for_a_batch_file() {
    assert_eq!(quote_for_cmd("%PATH%", true), "^^^%PATH^^^%");
    assert_eq!(quote_for_cmd("a b", true), r#"^^^"a^^^ b^^^""#);
}

#[test]
fn bash_quoting_keeps_a_windows_path_intact() {
    let arg = r"C:\Program Files\tool\";
    assert_eq!(
        build_command("node", &[arg.to_string()], ArgQuoting::Posix),
        r"node 'C:\Program Files\tool\'",
    );
    assert_eq!(
        build_command("node", &[arg.to_string()], CMD),
        r#"node ^"C:\Program^ Files\tool\\^""#,
    );
}

#[test]
fn cmd_quoting_escapes_twice_when_the_script_starts_with_a_batch_file() {
    let dir = tempdir().expect("temp dir");
    let batch_file = dir.path().join("tool.cmd");
    fs::write(&batch_file, "").expect("write the batch file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&batch_file, fs::Permissions::from_mode(0o755))
            .expect("make the batch file executable");
    }
    let search_path = dir.path().as_os_str();
    let quoting = |script: &str| ArgQuoting::cmd(script, search_path, dir.path());

    assert_eq!(quoting("tool.cmd --flag"), ArgQuoting::Cmd { double_escape: true });
    assert_eq!(
        quoting(&format!(r#""{}" --flag"#, batch_file.display())),
        ArgQuoting::Cmd { double_escape: true },
    );
    assert_eq!(quoting("node tool.cmd"), ArgQuoting::Cmd { double_escape: false });
    assert_eq!(
        ArgQuoting::cmd("missing.BAT", OsStr::new(""), dir.path()),
        ArgQuoting::Cmd { double_escape: true },
    );
}
