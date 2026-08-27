use super::with_boolean_negations;
use crate::cli_args::CliArgs;
use clap::{CommandFactory, FromArgMatches};

/// Resolve a boolean flag on the `install` subcommand after parsing
/// `argv` through the negation-augmented command, exercising the same
/// path `main` uses.
fn install_flag(argv: &[&str], flag_id: &str) -> Result<bool, clap::Error> {
    let matches = with_boolean_negations(CliArgs::command()).try_get_matches_from(argv)?;
    let (name, install) = matches.subcommand().expect("a subcommand");
    assert_eq!(name, "install");
    Ok(install.get_flag(flag_id))
}

/// Parse `argv` through the negation-augmented command all the way back
/// into `CliArgs`, so top-level (global) flags can be inspected.
fn parse(argv: &[&str]) -> Result<CliArgs, clap::Error> {
    with_boolean_negations(CliArgs::command())
        .try_get_matches_from(argv)
        .and_then(|matches| CliArgs::from_arg_matches(&matches))
}

#[test]
fn no_frozen_lockfile_parses_and_leaves_the_flag_off() {
    let frozen = install_flag(&["pnpm", "install", "--no-frozen-lockfile"], "frozen_lockfile")
        .expect("--no-frozen-lockfile should parse");
    assert!(!frozen);
}

#[test]
fn positive_frozen_lockfile_still_sets_the_flag() {
    let frozen = install_flag(&["pnpm", "install", "--frozen-lockfile"], "frozen_lockfile")
        .expect("--frozen-lockfile should parse");
    assert!(frozen);
}

#[test]
fn last_negation_wins_over_earlier_positive() {
    let frozen = install_flag(
        &["pnpm", "install", "--frozen-lockfile", "--no-frozen-lockfile"],
        "frozen_lockfile",
    )
    .expect("both flags together should parse");
    assert!(!frozen);
}

#[test]
fn last_positive_wins_over_earlier_negation() {
    let frozen = install_flag(
        &["pnpm", "install", "--no-frozen-lockfile", "--frozen-lockfile"],
        "frozen_lockfile",
    )
    .expect("both flags together should parse");
    assert!(frozen);
}

#[test]
fn negation_covers_other_boolean_flags() {
    for (argv, id) in [
        (["pnpm", "install", "--no-lockfile-only"], "lockfile_only"),
        (["pnpm", "install", "--no-dry-run"], "dry_run"),
        (["pnpm", "install", "--no-ignore-scripts"], "ignore_scripts"),
    ] {
        let value = install_flag(&argv, id).expect("boolean negation should parse");
        assert!(!value, "{id} should be off");
    }
}

#[test]
fn explicit_negations_are_not_shadowed() {
    let value = install_flag(
        &["pnpm", "install", "--no-prefer-frozen-lockfile"],
        "no_prefer_frozen_lockfile",
    )
    .expect("the hand-written --no-prefer-frozen-lockfile should still parse");
    assert!(value);
}

#[test]
fn genuinely_unknown_flag_still_errors() {
    let error = install_flag(&["pnpm", "install", "--no-such-flag"], "frozen_lockfile")
        .expect_err("an unknown flag must still be rejected");
    assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
}

#[test]
fn global_flag_negation_propagates_into_subcommands() {
    // `--recursive` is a global bool defined on the root command; its
    // generated `--no-recursive` must be global too so it parses under a
    // subcommand and resolves the flag off (the `is_global_set()` branch).
    let off = parse(&["pnpm", "install", "--no-recursive"]).expect("--no-recursive should parse");
    assert!(!off.recursive);
    let on = parse(&["pnpm", "install", "--recursive"]).expect("--recursive should parse");
    assert!(on.recursive);
    let toggled = parse(&["pnpm", "install", "--recursive", "--no-recursive"])
        .expect("both flags together should parse");
    assert!(!toggled.recursive);
}
