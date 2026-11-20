use super::{
    MissingWithCurrentCommand,
    plan,
};
use std::ffi::OsString;

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

    for flag in ["--color", "--yes", "--workspace-root", "-rw"] {
        let (rewritten, force) = plan(argv(&[flag, "with", "current", "--version"])).expect("plan");
        assert!(force);
        assert_eq!(strings(&rewritten), vec!["pnpm", flag, "--version"]);
    }
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
fn does_not_rewrite_with_current_inside_another_subcommand() {
    // `with current` as data for another command must be left alone, only
    // `with` at the subcommand position is the sugar.
    for tokens in [
        vec!["exec", "with", "current", "install"],
        vec!["run", "with", "current"],
        vec!["dlx", "with", "current", "foo"],
    ] {
        let original = argv(&tokens);
        let (rewritten, force) = plan(original.clone()).expect("plan");
        assert!(!force, "`with current` as an argument must not force pmOnFail: {tokens:?}");
        assert_eq!(strings(&rewritten), strings(&original), "argv must be untouched: {tokens:?}");
    }
}

#[test]
fn rewrites_with_current_after_a_value_taking_global_flag() {
    // `--reporter ndjson` consumes its value, so `with` is the subcommand.
    let (rewritten, force) =
        plan(argv(&["--reporter", "ndjson", "with", "current", "install"])).expect("plan");
    assert!(force);
    assert_eq!(strings(&rewritten), vec!["pnpm", "--reporter", "ndjson", "install"]);
}

#[test]
fn rewrites_with_current_after_a_clustered_value_taking_short() {
    // `-rC` is `-r -C`, so the directory is the value and `with` the command.
    let (rewritten, force) =
        plan(argv(&["-rC", "packages/foo", "with", "current", "install"])).expect("plan");
    assert!(force);
    assert_eq!(strings(&rewritten), vec!["pnpm", "-rC", "packages/foo", "install"]);
}

#[test]
fn rewrites_with_current_after_subcommand_boolean_and_setting_flags() {
    for (tokens, expected) in [
        (["--bare", "with", "current", "init"].as_slice(), ["pnpm", "--bare", "init"].as_slice()),
        (&["--prod", "with", "current", "install"], &["pnpm", "--prod", "install"]),
        (
            &["--shamefully-hoist", "with", "current", "install"],
            &["pnpm", "--shamefully-hoist", "install"],
        ),
        (&["pm", "with", "current", "list"], &["pnpm", "pm", "list"]),
        (&["recursive", "with", "current", "list"], &["pnpm", "recursive", "list"]),
    ] {
        let (rewritten, force) = plan(argv(tokens)).expect("plan");
        assert!(force, "tokens: {tokens:?}");
        assert_eq!(strings(&rewritten), expected, "tokens: {tokens:?}");
    }
}

#[test]
fn skips_with_after_separator_or_short_value_option() {
    for tokens in [
        ["-C", "with", "current", "install"].as_slice(),
        ["--", "with", "current", "install"].as_slice(),
    ] {
        let original = argv(tokens);
        let (rewritten, force) = plan(original.clone()).expect("plan");
        assert!(!force, "tokens: {tokens:?}");
        assert_eq!(strings(&rewritten), strings(&original), "tokens: {tokens:?}");
    }
}
