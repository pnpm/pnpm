use super::relocate_pre_subcommand_flags;
use crate::{boolean_negations::with_boolean_negations, cli_args::CliArgs};
use clap::{CommandFactory, FromArgMatches};
use pretty_assertions::assert_eq;
use std::ffi::OsString;

fn relocate(tokens: &[&str]) -> Vec<String> {
    let cmd = with_boolean_negations(CliArgs::command());
    relocate_pre_subcommand_flags(&cmd, tokens.iter().map(OsString::from).collect())
        .into_iter()
        .map(|token| token.into_string().expect("test tokens are UTF-8"))
        .collect()
}

fn parse(tokens: &[&str]) -> CliArgs {
    let cmd = with_boolean_negations(CliArgs::command());
    let argv = relocate_pre_subcommand_flags(&cmd, tokens.iter().map(OsString::from).collect());
    cmd.try_get_matches_from(argv)
        .and_then(|matches| CliArgs::from_arg_matches(&matches))
        .expect("parses after relocation")
}

#[test]
fn subcommand_flags_before_the_subcommand_move_after_it() {
    // The exact shape pnpm's `bundle-deps.ts` forwards to `pnpm deploy`
    // during a release (`--config.*` tokens already extracted).
    assert_eq!(
        relocate(&[
            "pnpm",
            "--ignore-scripts",
            "--force",
            "--filter=pnpm",
            "--prod",
            "deploy",
            "temp-deploy",
        ]),
        ["pnpm", "--filter=pnpm", "deploy", "--ignore-scripts", "--force", "--prod", "temp-deploy",],
        "subcommand flags move after `deploy` in order; the global --filter stays put",
    );
}

#[test]
fn relocated_deploy_invocation_parses_with_the_flags_applied() {
    let args = parse(&[
        "pnpm",
        "--ignore-scripts",
        "--force",
        "--filter=pnpm",
        "--prod",
        "deploy",
        "temp-deploy",
    ]);
    assert_eq!(args.filter, ["pnpm"]);
    let crate::cli_args::cli_command::CliCommand::Deploy(deploy) = args.command else {
        panic!("expected deploy");
    };
    assert!(deploy.force);
    assert!(deploy.install_args.ignore_scripts);
    assert_eq!(deploy.target_dirs, [std::path::PathBuf::from("temp-deploy")]);
}

#[test]
fn top_level_options_stay_in_place() {
    let argv = ["pnpm", "-C", "project", "--reporter", "ndjson", "install"];
    assert_eq!(relocate(&argv), argv, "every token is top-level grammar already");
}

#[test]
fn value_consuming_subcommand_option_moves_with_its_value() {
    assert_eq!(
        relocate(&["pnpm", "--tag", "next-11", "publish"]),
        ["pnpm", "publish", "--tag", "next-11"],
    );
}

#[test]
fn inline_value_moves_as_one_token() {
    assert_eq!(
        relocate(&["pnpm", "--node-linker=hoisted", "install"]),
        ["pnpm", "install", "--node-linker=hoisted"],
    );
}

#[test]
fn boolean_negations_move_like_their_positive_forms() {
    assert_eq!(
        relocate(&["pnpm", "--no-frozen-lockfile", "install"]),
        ["pnpm", "install", "--no-frozen-lockfile"],
    );
}

#[test]
fn short_subcommand_flag_moves() {
    assert_eq!(relocate(&["pnpm", "-P", "install"]), ["pnpm", "install", "-P"]);
}

#[test]
fn subcommand_alias_is_recognized() {
    assert_eq!(relocate(&["pnpm", "--prod", "i"]), ["pnpm", "i", "--prod"]);
}

#[test]
fn external_command_argv_is_untouched() {
    let argv = ["pnpm", "--ignore-scripts", "some-script"];
    assert_eq!(relocate(&argv), argv, "not a subcommand → script argv must not be reshaped");
}

#[test]
fn double_dash_terminator_stops_relocation() {
    let argv = ["pnpm", "--ignore-scripts", "--", "install"];
    assert_eq!(relocate(&argv), argv);
}

#[test]
fn argv_without_subcommand_flags_is_untouched() {
    let argv = ["pnpm", "install", "--frozen-lockfile"];
    assert_eq!(relocate(&argv), argv);
}
