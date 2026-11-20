use super::strip_prefix;
use pretty_assertions::assert_eq;
use std::ffi::OsString;

fn stripped(tokens: &[&str]) -> (Vec<String>, bool) {
    let (argv, forced) = strip_prefix(
        tokens
            .iter()
            .map(OsString::from)
            .collect(),
    );
    let argv = argv
        .into_iter()
        .map(OsString::into_string)
        .map(|text| text.expect("test tokens are UTF-8"))
        .collect();
    (argv, forced)
}

#[test]
fn a_leading_pm_token_is_stripped() {
    let (argv, forced) = stripped(&["pnpm", "pm", "clean", "--lockfile"]);
    assert_eq!(argv, ["pnpm", "clean", "--lockfile"]);
    assert!(forced, "the built-in command is forced");
}

#[test]
fn a_pm_after_pre_command_options_or_with_current_is_stripped() {
    for (tokens, expected) in [
        (
            ["pnpm", "--env-file", ".env", "pm", "list"].as_slice(),
            ["pnpm", "--env-file", ".env", "list"].as_slice(),
        ),
        (&["pnpm", "--dir", "/tmp/pkg", "pm", "clean"], &["pnpm", "--dir", "/tmp/pkg", "clean"]),
        (&["pnpm", "-rC", "/tmp/pkg", "pm", "clean"], &["pnpm", "-rC", "/tmp/pkg", "clean"]),
        (&["pnpm", "--lockfile", "pm", "clean"], &["pnpm", "--lockfile", "clean"]),
        (&["pnpm", "with", "current", "pm", "list"], &["pnpm", "with", "current", "list"]),
        (&["pnpm", "recursive", "pm", "list"], &["pnpm", "recursive", "list"]),
        (
            &["pnpm", "--dir", "/tmp/pkg", "with", "current", "pm", "list"],
            &["pnpm", "--dir", "/tmp/pkg", "with", "current", "list"],
        ),
    ] {
        let (argv, forced) = stripped(tokens);
        assert_eq!(argv, expected, "tokens: {tokens:?}");
        assert!(forced, "tokens: {tokens:?}");
    }
}

#[test]
fn a_bare_pm_leaves_no_command() {
    let (argv, forced) = stripped(&["pnpm", "pm"]);
    assert_eq!(argv, ["pnpm"]);
    assert!(forced);
}

#[test]
fn a_pm_elsewhere_on_the_command_line_is_an_ordinary_argument() {
    for tokens in [
        ["pnpm", "run", "pm"].as_slice(),
        ["pnpm", "--dir", "pm", "clean"].as_slice(),
        ["pnpm", "-C", "pm", "clean"].as_slice(),
        ["pnpm", "exec", "pm", "clean"].as_slice(),
        ["pnpm", "with", "10", "pm", "clean"].as_slice(),
        ["pnpm", "--", "pm", "clean"].as_slice(),
    ] {
        let (argv, forced) = stripped(tokens);
        assert_eq!(argv, tokens, "tokens: {tokens:?}");
        assert!(!forced, "tokens: {tokens:?}");
    }
}

#[test]
fn an_argv_without_a_command_is_left_alone() {
    let (argv, forced) = stripped(&["pnpm"]);
    assert_eq!(argv, ["pnpm"]);
    assert!(!forced);
}
