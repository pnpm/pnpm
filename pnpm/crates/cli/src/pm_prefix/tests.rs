use super::strip_prefix;
use pretty_assertions::assert_eq;
use std::ffi::OsString;

fn stripped(tokens: &[&str]) -> (Vec<String>, bool) {
    let (argv, forced) = strip_prefix(tokens.iter().map(OsString::from).collect());
    let argv =
        argv.into_iter().map(|token| token.into_string().expect("test tokens are UTF-8")).collect();
    (argv, forced)
}

#[test]
fn a_leading_pm_token_is_stripped() {
    let (argv, forced) = stripped(&["pnpm", "pm", "clean", "--lockfile"]);
    assert_eq!(argv, ["pnpm", "clean", "--lockfile"]);
    assert!(forced, "the built-in command is forced");
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
        ["pnpm", "exec", "pm", "clean"].as_slice(),
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
