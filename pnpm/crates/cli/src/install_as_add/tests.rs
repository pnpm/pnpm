use super::rewrite;
use crate::{
    boolean_negations::with_boolean_negations,
    cli_args::{CliArgs, cli_command::CliCommand},
    config_overrides::ConfigOverrides,
    flag_relocation::relocate_pre_subcommand_flags,
};
use clap::{CommandFactory, FromArgMatches};
use pnpm_config::Config;
use pretty_assertions::assert_eq;
use std::{ffi::OsString, path::Path};

fn rewritten(tokens: &[&str]) -> Vec<String> {
    let cmd = with_boolean_negations(CliArgs::command());
    let argv = relocate_pre_subcommand_flags(&cmd, tokens.iter().map(OsString::from).collect());
    rewrite(&cmd, argv)
        .into_iter()
        .map(|token| token.into_string().expect("test tokens are UTF-8"))
        .collect()
}

fn parse(tokens: &[&str]) -> CliArgs {
    parse_argv(tokens.iter().map(OsString::from).collect())
}

fn parse_argv(argv: Vec<OsString>) -> CliArgs {
    let cmd = with_boolean_negations(CliArgs::command());
    let argv = relocate_pre_subcommand_flags(&cmd, argv);
    let argv = rewrite(&cmd, argv);
    cmd.try_get_matches_from(argv)
        .and_then(|matches| CliArgs::from_arg_matches(&matches))
        .expect("parses after the rewrite")
}

#[test]
fn install_with_a_package_name_becomes_add() {
    for command in ["install", "i"] {
        let argv = rewritten(&["pnpm", command, "valibot"]);
        assert_eq!(argv, ["pnpm", "add", "valibot"], "command: {command}");
    }
}

#[test]
fn install_without_a_package_name_stays_install() {
    assert_eq!(rewritten(&["pnpm", "install"]), ["pnpm", "install"]);
    assert_eq!(
        rewritten(&["pnpm", "install", "--frozen-lockfile"]),
        ["pnpm", "install", "--frozen-lockfile"],
    );
}

#[test]
fn a_value_taking_option_is_not_mistaken_for_a_package_name() {
    assert_eq!(
        rewritten(&["pnpm", "install", "--reporter", "silent"]),
        ["pnpm", "install", "--reporter", "silent"],
    );
}

#[test]
fn install_test_is_left_alone() {
    assert_eq!(rewritten(&["pnpm", "install-test"]), ["pnpm", "install-test"]);
}

#[test]
fn a_package_name_after_the_separator_becomes_add() {
    assert_eq!(
        rewritten(&["pnpm", "install", "--lockfile-only", "--", "valibot"]),
        ["pnpm", "add", "--lockfile-only", "--", "valibot"],
    );
}

#[test]
fn a_trailing_separator_alone_stays_install() {
    assert_eq!(rewritten(&["pnpm", "install", "--"]), ["pnpm", "install", "--"]);
}

#[test]
fn a_recursive_install_with_a_package_name_becomes_add() {
    assert_eq!(
        rewritten(&["pnpm", "recursive", "install", "valibot"]),
        ["pnpm", "--recursive", "add", "valibot"],
    );
}

#[test]
fn the_rewritten_invocation_parses_as_add() {
    let args = parse(&["pnpm", "i", "-D", "valibot", "vitest"]);

    let CliCommand::Add(add) = args.command else {
        panic!("expected add");
    };
    assert_eq!(add.package_names, ["valibot", "vitest"]);
}

#[test]
fn the_separator_spelling_parses_as_add() {
    let args = parse(&["pnpm", "install", "--", "valibot"]);

    let CliCommand::Add(add) = args.command else {
        panic!("expected add");
    };
    assert_eq!(add.package_names, ["valibot"]);
}

/// `--offline` is `install`'s own option and not `add`'s, so on a command
/// line that names a package it must come off argv as the setting before
/// the rewrite hands the rest to `add`'s grammar.
#[test]
fn install_with_offline_after_the_package_parses_as_add() {
    let (overrides, argv) = ConfigOverrides::extract(
        ["pnpm", "install", "valibot", "--offline", "--ignore-scripts"].map(OsString::from),
    );
    let args = parse_argv(argv);

    let CliCommand::Add(add) = args.command else {
        panic!("expected add");
    };
    assert_eq!(add.package_names, ["valibot"]);
    assert!(add.ignore_scripts);
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(config.offline);
}
