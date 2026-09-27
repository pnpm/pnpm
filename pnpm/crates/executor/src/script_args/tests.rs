use super::{ArgQuoting, build_command, posix_quote, quote_for_cmd};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};
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

fn write_batch_file(dir: &Path) -> PathBuf {
    let batch_file = dir.join("tool.cmd");
    fs::write(&batch_file, "").expect("write the batch file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&batch_file, fs::Permissions::from_mode(0o755))
            .expect("make the batch file executable");
    }
    batch_file
}

#[test]
fn cmd_quoting_escapes_twice_only_when_the_script_ends_with_a_batch_file() {
    let wd = tempdir().expect("temp dir");
    let bin_dir = wd.path().join("bin");
    fs::create_dir(&bin_dir).expect("create the bin dir");
    let batch_file = write_batch_file(&bin_dir);
    let cases = [
        ("tool.cmd --flag".to_string(), true),
        ("tool.cmd\t--flag".to_string(), true),
        (format!(r#""{}" --flag"#, batch_file.display()), true),
        ("bin/tool.cmd --flag".to_string(), true),
        ("echo ready && tool.cmd".to_string(), true),
        ("tool.cmd 2>&1".to_string(), true),
        ("tool.cmd <&0".to_string(), true),
        ("echo ^>& tool.cmd".to_string(), true),
        ("echo ^<& tool.cmd".to_string(), true),
        ("node x.js ^& tool.cmd".to_string(), false),
        ("tool.cmd && node x.js".to_string(), false),
        ("node tool.cmd".to_string(), false),
    ];
    for (script, double_escape) in cases {
        assert_eq!(
            ArgQuoting::cmd(&script, bin_dir.as_os_str(), wd.path()),
            ArgQuoting::Cmd { double_escape },
            "quoting for {script:?}",
        );
    }
    assert_eq!(
        ArgQuoting::cmd("missing.BAT", OsStr::new(""), wd.path()),
        ArgQuoting::Cmd { double_escape: true },
    );
}

#[test]
fn cmd_quoting_finds_a_batch_file_in_the_directory_the_script_runs_in() {
    let wd = tempdir().expect("temp dir");
    write_batch_file(wd.path());
    assert_eq!(
        ArgQuoting::cmd("tool.cmd", OsStr::new(""), wd.path()),
        ArgQuoting::Cmd { double_escape: true },
    );
}
