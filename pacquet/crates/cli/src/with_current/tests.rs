use super::{MissingWithCurrentCommand, long_option_consumes_value, plan};
use std::ffi::OsString;

/// Build an argv (with a leading program name) from string slices.
fn argv(tokens: &[&str]) -> Vec<OsString> {
    std::iter::once("pnpm").chain(tokens.iter().copied()).map(OsString::from).collect()
}

fn strings(argv: &[OsString]) -> Vec<String> {
    argv.iter().map(|token| token.to_string_lossy().into_owned()).collect()
}

#[test]
fn strips_the_with_current_tokens_and_flags_the_override() {
    let (rewritten, force) = plan(argv(&["with", "current", "install"])).expect("plan");
    assert!(force, "a stripped `with current` forces pmOnFail=ignore");
    assert_eq!(strings(&rewritten), vec!["pnpm", "install"]);
}

#[test]
fn forwards_the_command_arguments() {
    let (rewritten, _) =
        plan(argv(&["with", "current", "add", "foo", "--save-dev"])).expect("plan");
    assert_eq!(strings(&rewritten), vec!["pnpm", "add", "foo", "--save-dev"]);
}

#[test]
fn preserves_global_flags_before_with() {
    // A boolean global flag (`-r` long form) before `with` is kept.
    let (rewritten, force) =
        plan(argv(&["--recursive", "with", "current", "install"])).expect("plan");
    assert!(force);
    assert_eq!(strings(&rewritten), vec!["pnpm", "--recursive", "install"]);
}

#[test]
fn leaves_argv_untouched_without_with_current() {
    let (rewritten, force) = plan(argv(&["with", "10", "install"])).expect("plan");
    assert!(!force, "a version spec is not the `current` sugar");
    assert_eq!(strings(&rewritten), vec!["pnpm", "with", "10", "install"]);

    let (rewritten, force) = plan(argv(&["install"])).expect("plan");
    assert!(!force);
    assert_eq!(strings(&rewritten), vec!["pnpm", "install"]);
}

#[test]
fn errors_when_no_command_follows_current() {
    let error = plan(argv(&["with", "current"])).expect_err("missing command must error");
    assert!(error.downcast_ref::<MissingWithCurrentCommand>().is_some());
}

#[test]
fn skips_a_with_consumed_by_a_value_taking_option() {
    // `--reporter with` makes `with` the reporter's value, so this
    // `with current` pair is not the command and must not be rewritten.
    let (rewritten, force) =
        plan(argv(&["--reporter", "with", "current", "install"])).expect("plan");
    assert!(!force);
    assert_eq!(strings(&rewritten), vec!["pnpm", "--reporter", "with", "current", "install"]);
}

#[test]
fn long_option_value_consumption_matches_pnpm() {
    // Value-takers and unknown options consume their successor.
    assert!(long_option_consumes_value(&OsString::from("--reporter")));
    assert!(long_option_consumes_value(&OsString::from("--unknown-flag")));
    // Booleans, `--no-` negations, and inline `=` values do not.
    assert!(!long_option_consumes_value(&OsString::from("--recursive")));
    assert!(!long_option_consumes_value(&OsString::from("--no-something")));
    assert!(!long_option_consumes_value(&OsString::from("--reporter=ndjson")));
    // Short options aren't long options.
    assert!(!long_option_consumes_value(&OsString::from("-r")));
}
