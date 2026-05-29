use super::{CliArgs, CliCommand};
use clap::Parser;

/// `--recursive` / `-r` defaults to `false` when absent. Mirrors
/// pnpm, where `recursive` is unset unless the flag (or a recursive
/// command form) is used.
#[test]
fn recursive_default_is_false() {
    let parsed = CliArgs::try_parse_from(["pacquet", "install"]).expect("parses");
    assert!(!parsed.recursive, "flag absent → false");
}

/// `-r` is a global flag, so it parses both before and after the
/// subcommand. Mirrors pnpm's global `-r` / `--recursive`.
#[test]
fn recursive_flag_is_global_and_parses_either_side_of_subcommand() {
    let before = CliArgs::try_parse_from(["pacquet", "-r", "install"]).expect("parses -r install");
    assert!(before.recursive, "`-r install` → recursive");
    assert!(matches!(before.command, CliCommand::Install(_)));

    let after = CliArgs::try_parse_from(["pacquet", "install", "--recursive"])
        .expect("parses install --recursive");
    assert!(after.recursive, "`install --recursive` → recursive");
    assert!(matches!(after.command, CliCommand::Install(_)));
}

/// `--filter` / `--filter-prod` default to empty when absent.
#[test]
fn filter_defaults_are_empty() {
    let parsed = CliArgs::try_parse_from(["pacquet", "install"]).expect("parses");
    assert!(parsed.filter.is_empty(), "no `--filter` → empty");
    assert!(parsed.filter_prod.is_empty(), "no `--filter-prod` → empty");
}

/// `--filter` (and its `-F` short form) is repeatable, collecting each
/// occurrence into the selector list, and `--filter-prod` collects
/// separately. Mirrors pnpm's CLI-only `filter` / `filterProd` arrays.
#[test]
fn filter_flags_collect_selectors() {
    let parsed = CliArgs::try_parse_from([
        "pacquet",
        "install",
        "--filter",
        "@scope/*",
        "-F",
        "./pkg",
        "--filter-prod",
        "app...",
    ])
    .expect("parses repeated filter flags");
    assert_eq!(parsed.filter, ["@scope/*", "./pkg"]);
    assert_eq!(parsed.filter_prod, ["app..."]);
    assert!(matches!(parsed.command, CliCommand::Install(_)));
}

/// `-F` is global, so it parses before the subcommand too. Mirrors
/// pnpm's global `--filter`.
#[test]
fn filter_flag_is_global_and_parses_before_subcommand() {
    let parsed = CliArgs::try_parse_from(["pacquet", "-F", "@scope/*", "install"])
        .expect("parses -F install");
    assert_eq!(parsed.filter, ["@scope/*"]);
    assert!(matches!(parsed.command, CliCommand::Install(_)));
}

/// Occurrences of a global repeatable flag collect within one side of
/// the subcommand boundary, but mixing sides keeps only the
/// subcommand-side occurrence — a clap limitation the `filter` field
/// docs warn about. Locks that behavior so the doc claim stays honest.
#[test]
fn filter_flag_split_across_subcommand_keeps_only_subcommand_side() {
    let parsed = CliArgs::try_parse_from(["pacquet", "-F", "a", "install", "-F", "b"])
        .expect("parses split -F");
    assert_eq!(parsed.filter, ["b"], "global-side `a` is dropped");
}
