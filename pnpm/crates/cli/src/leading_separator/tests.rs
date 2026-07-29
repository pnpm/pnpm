use super::preserve_leading_separator;
use pretty_assertions::assert_eq;
use std::ffi::OsString;

fn preserve<Items: IntoIterator<Item = &'static str>>(items: Items) -> Vec<String> {
    preserve_leading_separator(items.into_iter().map(OsString::from).collect())
        .into_iter()
        .map(|token| token.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn a_script_shortcut_opening_on_the_separator_keeps_it() {
    for command in ["test", "start", "stop"] {
        let argv = preserve(["pnpm", command, "--", "--flag"]);
        dbg!(command, &argv);
        assert_eq!(argv, ["pnpm", command, "--", "--", "--flag"], "command: {command}");
    }
}

#[test]
fn a_separator_after_the_first_argument_is_left_alone() {
    // The positional has already taken a value, so clap keeps this one.
    let argv = preserve(["pnpm", "stop", "x", "--", "y"]);

    assert_eq!(argv, ["pnpm", "stop", "x", "--", "y"]);
}

#[test]
fn a_run_script_name_before_the_separator_leaves_it_alone() {
    let argv = preserve(["pnpm", "run", "build", "--", "--flag"]);

    assert_eq!(argv, ["pnpm", "run", "build", "--", "--flag"]);
}

#[test]
fn global_options_before_the_command_are_stepped_over() {
    let argv = preserve(["pnpm", "--dir", "/tmp/project", "stop", "--", "--flag"]);

    assert_eq!(argv, ["pnpm", "--dir", "/tmp/project", "stop", "--", "--", "--flag"]);
}

#[test]
fn a_command_that_owns_its_arguments_is_left_alone() {
    // `install` parses its own argv, so the separator is clap's to consume.
    let argv = preserve(["pnpm", "install", "--", "--config.foo=bar"]);

    assert_eq!(argv, ["pnpm", "install", "--", "--config.foo=bar"]);
}

#[test]
fn argv_without_a_separator_is_unchanged() {
    let argv = preserve(["pnpm", "stop", "--flag"]);

    assert_eq!(argv, ["pnpm", "stop", "--flag"]);
}
