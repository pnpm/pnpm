use super::preserve_script_separator;
use crate::{boolean_negations::with_boolean_negations, cli_args::CliArgs};
use clap::CommandFactory;
use pretty_assertions::assert_eq;
use std::ffi::OsString;

fn preserve<Items: IntoIterator<Item = &'static str>>(items: Items) -> Vec<String> {
    let command = with_boolean_negations(CliArgs::command());
    let argv = items.into_iter().map(OsString::from).collect::<Vec<_>>();
    preserve_script_separator(&command, argv)
        .into_iter()
        .map(|token| token.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn a_leading_separator_is_doubled_so_the_script_still_receives_one() {
    let argv = preserve(["pnpm", "run", "build", "--", "--flag"]);

    assert_eq!(argv, ["pnpm", "run", "build", "--", "--", "--flag"]);
}

#[test]
fn a_separator_after_the_first_argument_is_left_alone() {
    // clap keeps this one, because the trailing arguments already started.
    let argv = preserve(["pnpm", "run", "build", "x", "--", "y"]);

    assert_eq!(argv, ["pnpm", "run", "build", "x", "--", "y"]);
}

#[test]
fn options_between_the_script_name_and_the_separator_are_stepped_over() {
    let argv = preserve(["pnpm", "run", "build", "--if-present", "--", "--flag"]);

    assert_eq!(argv, ["pnpm", "run", "build", "--if-present", "--", "--", "--flag"]);
}

#[test]
fn global_options_before_the_subcommand_are_stepped_over() {
    let argv = preserve(["pnpm", "-C", "/tmp/project", "run", "build", "--", "--flag"]);

    assert_eq!(argv, ["pnpm", "-C", "/tmp/project", "run", "build", "--", "--", "--flag"]);
}

#[test]
fn the_script_shortcuts_that_take_arguments_are_covered() {
    for command in ["stop", "restart"] {
        let argv = preserve(["pnpm", command, "--", "--flag"]);
        dbg!(command, &argv);
        assert_eq!(argv, ["pnpm", command, "--", "--", "--flag"], "command: {command}");
    }
}

#[test]
fn a_run_without_a_script_name_is_left_alone() {
    let argv = preserve(["pnpm", "run", "--", "--flag"]);

    assert_eq!(argv, ["pnpm", "run", "--", "--flag"]);
}

#[test]
fn commands_that_do_not_run_scripts_keep_their_separator_semantics() {
    let argv = preserve(["pnpm", "exec", "--", "node", "--version"]);

    assert_eq!(argv, ["pnpm", "exec", "--", "node", "--version"]);
}

#[test]
fn argv_without_a_separator_is_unchanged() {
    let argv = preserve(["pnpm", "run", "build", "--flag"]);

    assert_eq!(argv, ["pnpm", "run", "build", "--flag"]);
}
