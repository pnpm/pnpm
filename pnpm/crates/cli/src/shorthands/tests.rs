use super::expand_universal_shorthands;
use crate::{
    boolean_negations::with_boolean_negations,
    cli_args::{CliArgs, reporter::ReporterType},
    flag_relocation::relocate_pre_subcommand_flags,
};
use clap::{CommandFactory, FromArgMatches};
use pretty_assertions::assert_eq;
use std::ffi::OsString;

fn expand(tokens: &[&str]) -> Vec<String> {
    let cmd = with_boolean_negations(CliArgs::command());
    expand_universal_shorthands(&cmd, tokens.iter().map(OsString::from).collect())
        .into_iter()
        .map(|token| token.into_string().expect("test tokens are UTF-8"))
        .collect()
}

/// Run the full pre-parse pipeline (shorthands, then relocation) and parse.
fn parse(tokens: &[&str]) -> CliArgs {
    let cmd = with_boolean_negations(CliArgs::command());
    let argv = expand_universal_shorthands(&cmd, tokens.iter().map(OsString::from).collect());
    let argv = relocate_pre_subcommand_flags(&cmd, argv);
    cmd.try_get_matches_from(argv)
        .and_then(|matches| CliArgs::from_arg_matches(&matches))
        .expect("parses after shorthand expansion")
}

#[test]
fn silent_long_form_expands_for_any_command() {
    assert_eq!(
        expand(&["pnpm", "store", "path", "--silent"]),
        ["pnpm", "store", "path", "--reporter=silent"],
    );
    assert_eq!(expand(&["pnpm", "install", "--silent"]), ["pnpm", "install", "--reporter=silent"]);
    // `run` overrides only the short `s`; the long `--silent` stays universal.
    assert_eq!(
        expand(&["pnpm", "run", "build", "--silent"]),
        ["pnpm", "run", "build", "--reporter=silent"],
    );
}

#[test]
fn short_s_expands_for_commands_that_do_not_own_it() {
    assert_eq!(expand(&["pnpm", "install", "-s"]), ["pnpm", "install", "--reporter=silent"]);
    // Pre-subcommand placement expands too — nopt is position-independent.
    assert_eq!(expand(&["pnpm", "-s", "install"]), ["pnpm", "--reporter=silent", "install"]);
    assert_eq!(expand(&["pnpm", "test", "-s"]), ["pnpm", "test", "--reporter=silent"]);
}

#[test]
fn short_s_is_left_for_run_which_defines_sequential() {
    assert_eq!(expand(&["pnpm", "run", "-s", "build"]), ["pnpm", "run", "-s", "build"]);
    assert_eq!(expand(&["pnpm", "-s", "run", "build"]), ["pnpm", "-s", "run", "build"]);
    // `recursive run` resolves to `run` as well.
    assert_eq!(
        expand(&["pnpm", "recursive", "run", "build", "-s"]),
        ["pnpm", "recursive", "run", "build", "-s"],
    );
}

#[test]
fn short_s_is_left_for_the_script_fallback() {
    // `pnpm <script>` dispatches through `run`, which inherits its
    // shorthand table in pnpm.
    assert_eq!(expand(&["pnpm", "my-script", "-s"]), ["pnpm", "my-script", "-s"]);
}

#[test]
fn tokens_after_the_terminator_are_untouched() {
    assert_eq!(
        expand(&["pnpm", "exec", "--", "-s", "--silent"]),
        ["pnpm", "exec", "--", "-s", "--silent"],
    );
}

#[test]
fn option_values_are_not_rewritten() {
    // `--filter` consumes the next token; a value spelling `-s` stays a value.
    assert_eq!(
        expand(&["pnpm", "--filter", "-s", "install"]),
        ["pnpm", "--filter", "-s", "install"],
    );
}

#[test]
fn expanded_silent_parses_to_the_silent_reporter() {
    let args = parse(&["pnpm", "store", "path", "--silent"]);
    assert!(matches!(args.reporter, ReporterType::Silent));

    let args = parse(&["pnpm", "install", "-s"]);
    assert!(matches!(args.reporter, ReporterType::Silent));
}

#[test]
fn later_reporter_overrides_the_silent_shorthand() {
    // nopt takes the last occurrence of a repeated option; `--silent` is
    // sugar for `--reporter=silent`, so an explicit later `--reporter` wins.
    let args = parse(&["pnpm", "install", "--silent", "--reporter=ndjson"]);
    assert!(matches!(args.reporter, ReporterType::Ndjson));

    let args = parse(&["pnpm", "install", "--reporter=ndjson", "--silent"]);
    assert!(matches!(args.reporter, ReporterType::Silent));
}
