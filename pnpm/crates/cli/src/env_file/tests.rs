use super::{load, scan, strip_flags};
use std::{ffi::OsString, path::PathBuf};

/// Build an argv (with a leading program name) from string slices.
fn argv(tokens: &[&str]) -> Vec<OsString> {
    std::iter::once("pnpm")
        .chain(tokens.iter().copied())
        .map(OsString::from)
        .collect()
}

fn strings(argv: &[OsString]) -> Vec<String> {
    argv.iter()
        .map(|token| token.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn strips_the_separate_value_form() {
    let (paths, remaining) = scan(&argv(&["--env-file", ".env", "install"]));
    assert_eq!(paths, vec![PathBuf::from(".env")]);
    assert_eq!(strings(&remaining), vec!["pnpm", "install"]);
}

#[test]
fn strips_the_inline_value_form() {
    let (paths, remaining) = scan(&argv(&["--env-file=.env", "install"]));
    assert_eq!(paths, vec![PathBuf::from(".env")]);
    assert_eq!(strings(&remaining), vec!["pnpm", "install"]);
}

#[test]
fn collects_repeated_flags_in_order() {
    let (paths, remaining) = scan(&argv(&["--env-file", "a.env", "--env-file=b.env", "install"]));
    assert_eq!(paths, vec![PathBuf::from("a.env"), PathBuf::from("b.env")]);
    assert_eq!(strings(&remaining), vec!["pnpm", "install"]);
}

#[test]
fn accepts_the_flag_after_the_subcommand() {
    let (paths, remaining) = scan(&argv(&["install", "--env-file", ".env"]));
    assert_eq!(paths, vec![PathBuf::from(".env")]);
    assert_eq!(strings(&remaining), vec!["pnpm", "install"]);
}

#[test]
fn leaves_script_arguments_alone() {
    for tokens in [
        vec!["run", "build", "--env-file", ".env"],
        vec!["exec", "cmd", "--env-file=.env"],
        vec!["install", "--", "--env-file", ".env"],
    ] {
        let argv = argv(&tokens);
        let (paths, remaining) = scan(&argv);
        assert!(paths.is_empty(), "claims a script argument: {tokens:?}");
        assert_eq!(remaining, argv, "rewrites a script argument: {tokens:?}");
    }
}

#[test]
fn leaves_a_valueless_flag_for_clap_to_report() {
    let valueless = argv(&["--env-file"]);
    let (paths, remaining) = scan(&valueless);
    assert!(paths.is_empty());
    assert_eq!(remaining, valueless);

    let followed_by_option = argv(&["--env-file", "--verbose", "install"]);
    let (paths, remaining) = scan(&followed_by_option);
    assert!(paths.is_empty(), "claims an option as the path");
    assert_eq!(remaining, followed_by_option);
}

#[test]
fn does_not_claim_the_flag_as_another_options_value() {
    // `--dir` consumes `--env-file` as its value, so the scan must not
    // treat it as the flag.
    let argv = argv(&["--dir", "--env-file", "install"]);
    let (paths, remaining) = scan(&argv);
    assert!(paths.is_empty());
    assert_eq!(remaining, argv);
}

#[test]
fn strips_the_flag_after_the_pm_prefix() {
    // `pm` forces the built-in command but is not a subcommand, so the
    // boundary must be computed as if it were absent.
    let (paths, remaining) = scan(&argv(&["pm", "install", "--env-file", ".env"]));
    assert_eq!(paths, vec![PathBuf::from(".env")]);
    assert_eq!(strings(&remaining), vec!["pnpm", "pm", "install"]);
}

#[test]
fn claims_the_flag_when_pm_is_not_the_prefix() {
    // `pm` past the first token is an ordinary argument (here: a package
    // name), so `--env-file` is still pnpm's own flag.
    let (paths, remaining) = scan(&argv(&["install", "pm", "--env-file", ".env"]));
    assert_eq!(paths, vec![PathBuf::from(".env")]);
    assert_eq!(strings(&remaining), vec!["pnpm", "install", "pm"]);
}

#[cfg(unix)]
#[test]
fn claims_a_non_utf8_value() {
    use std::os::unix::ffi::OsStringExt;
    let raw = vec![0x66, 0x6F, 0xFF];
    let mut owned = argv(&["--env-file", "placeholder", "install"]);
    owned[2] = OsString::from_vec(raw.clone());
    let (paths, remaining) = scan(&owned);
    assert_eq!(paths, vec![PathBuf::from(OsString::from_vec(raw))]);
    assert_eq!(strings(&remaining), vec!["pnpm", "install"]);
}

#[cfg(unix)]
#[test]
fn claims_an_inline_non_utf8_value() {
    use std::os::unix::ffi::OsStringExt;
    let path_bytes = vec![0x66, 0x6F, 0xFF];
    let mut flag_bytes = b"--env-file=".to_vec();
    flag_bytes.extend_from_slice(&path_bytes);
    let mut owned = argv(&["placeholder", "install"]);
    owned[1] = OsString::from_vec(flag_bytes);
    let (paths, remaining) = scan(&owned);
    assert_eq!(paths, vec![PathBuf::from(OsString::from_vec(path_bytes))]);
    assert_eq!(strings(&remaining), vec!["pnpm", "install"]);
}

#[cfg(unix)]
#[test]
fn steps_over_an_attached_non_utf8_short_option_value() {
    use std::os::unix::ffi::OsStringExt;
    let mut owned = argv(&["placeholder", "install", "--env-file", ".env"]);
    owned[1] = OsString::from_vec(b"-C\xFF".to_vec());
    let (paths, remaining) = scan(&owned);
    assert_eq!(paths, vec![PathBuf::from(".env")]);
    assert_eq!(remaining.len(), 3);
    assert_eq!(remaining[0], "pnpm");
    assert_eq!(remaining[2], "install");
}

#[test]
fn pnpx_alias_with_package_named_like_subcommand_keeps_env_file_for_the_package() {
    let rewritten = crate::inject_alias_subcommand(
        Some("pnpx"),
        vec![
            OsString::from("pnpx"),
            OsString::from("add"),
            OsString::from("--env-file"),
            OsString::from("x"),
        ],
    );
    let (paths, remaining) = scan(&rewritten);
    assert!(paths.is_empty());
    assert_eq!(remaining, rewritten);
}

#[test]
fn steps_over_a_clustered_short_option_taking_a_value() {
    let (paths, remaining) = scan(&argv(&["-rC", "project", "--env-file", ".env", "run", "build"]));
    assert_eq!(paths, vec![PathBuf::from(".env")]);
    assert_eq!(strings(&remaining), vec!["pnpm", "-rC", "project", "run", "build"]);
}

#[test]
fn strip_flags_keeps_everything_else() {
    let stripped =
        strip_flags(&argv(&["--recursive", "--env-file", "a.env", "install", "--env-file=b.env"]));
    assert_eq!(strings(&stripped), vec!["pnpm", "--recursive", "install"]);
}

#[test]
fn load_fails_loudly_on_a_missing_file() {
    let missing = PathBuf::from("pacquet-env-file-test-does-not-exist.env");
    let error =
        load(std::slice::from_ref(&missing), &mut |_, _| ()).expect_err("missing file must fail");
    let message = format!("{error:?}");
    assert!(
        message.contains("pacquet-env-file-test-does-not-exist.env"),
        "error names the file: {message}",
    );
}
