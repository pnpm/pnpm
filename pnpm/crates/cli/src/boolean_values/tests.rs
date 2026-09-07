use super::{boolean_flags, resolve_boolean_values, spellings};
use crate::{boolean_negations::with_boolean_negations, cli_args::CliArgs};
use clap::CommandFactory;
use pretty_assertions::assert_eq;
use std::ffi::OsString;

fn argv(parts: &[&str]) -> Vec<OsString> {
    parts.iter().map(OsString::from).collect()
}

fn resolve(parts: &[&str]) -> Vec<String> {
    resolve_boolean_values(argv(parts))
        .into_iter()
        .map(|token| token.to_string_lossy().into_owned())
        .collect()
}

/// Resolve a boolean flag on the `install` subcommand the way `main`
/// does: the pass, then the negation-augmented command.
fn install_flag(parts: &[&str], flag_id: &str) -> Result<bool, clap::Error> {
    let argv = resolve_boolean_values(argv(parts));
    let matches = with_boolean_negations(CliArgs::command()).try_get_matches_from(argv)?;
    let (name, install) = matches.subcommand().expect("a subcommand");
    assert_eq!(name, "install");
    Ok(install.get_flag(flag_id))
}

#[test]
fn a_false_value_turns_the_flag_off() {
    for spelling in ["--prod=false", "--prod=0"] {
        let prod = install_flag(&["pnpm", "install", spelling], "prod")
            .unwrap_or_else(|err| panic!("{spelling} should parse: {err}"));
        assert!(!prod, "{spelling} should leave --prod off");
    }
}

#[test]
fn a_true_value_turns_the_flag_on() {
    for spelling in ["--prod=true", "--prod=1"] {
        let prod = install_flag(&["pnpm", "install", spelling], "prod")
            .unwrap_or_else(|err| panic!("{spelling} should parse: {err}"));
        assert!(prod, "{spelling} should turn --prod on");
    }
}

/// A foreign command line names its program with a positional, so a
/// boolean token after a flag stays a positional. See the module docs.
#[test]
fn a_value_written_as_its_own_token_is_left_alone() {
    let original = ["pnpm", "install", "--prod", "false"];
    assert_eq!(resolve(&original), original);
    let exec = ["pnpm", "-r", "exec", "--report-summary", "true"];
    assert_eq!(resolve(&exec), exec);
}

#[test]
fn an_alias_resolves_to_the_flag_it_names() {
    let prod = install_flag(&["pnpm", "install", "--production=false"], "prod")
        .expect("--production=false should parse");
    assert!(!prod);
}

#[test]
fn a_negation_spelling_takes_a_value_too() {
    assert_eq!(
        resolve(&["pnpm", "install", "--no-optional=false"]),
        ["pnpm", "install", "--optional"],
    );
    assert_eq!(
        resolve(&["pnpm", "install", "--no-frozen-lockfile=true"]),
        ["pnpm", "install", "--no-frozen-lockfile"],
    );
}

/// `--no-runtime` is pnpm 12's only spelling of that flag: it has no
/// `--runtime` for a false value to resolve to.
#[test]
fn a_standalone_negation_takes_a_true_value_only() {
    assert_eq!(
        resolve(&["pnpm", "install", "--no-runtime=true"]),
        ["pnpm", "install", "--no-runtime"],
    );
    let unresolvable = ["pnpm", "install", "--no-runtime=false"];
    assert_eq!(resolve(&unresolvable), unresolvable);
}

#[test]
fn a_global_flag_takes_a_value_before_its_command() {
    assert_eq!(
        resolve(&["pnpm", "--recursive=false", "install"]),
        ["pnpm", "--no-recursive", "install"],
    );
}

#[test]
fn a_value_the_flag_does_not_take_is_left_for_clap() {
    let original = ["pnpm", "install", "--prod=maybe"];
    assert_eq!(resolve(&original), original);
    let err = install_flag(&original, "prod").expect_err("--prod=maybe should not parse");
    assert_eq!(err.kind(), clap::error::ErrorKind::TooManyValues);
}

#[test]
fn an_option_value_that_spells_a_boolean_flag_is_left_alone() {
    let original = ["pnpm", "install", "--filter", "--prod=false"];
    assert_eq!(resolve(&original), original);
}

#[test]
fn a_forwarded_boolean_value_reaches_the_script() {
    for original in [
        &["pnpm", "run", "build", "--prod=false"][..],
        &["pnpm", "exec", "cmd", "--prod=false"],
        &["pnpm", "install", "--", "--prod=false"],
    ] {
        assert_eq!(resolve(original), original);
    }
}

#[test]
fn a_setting_no_command_declares_is_left_to_the_settings_pass() {
    // `ConfigOverrides::extract` has already taken `--shamefully-hoist`
    // by the time this pass runs; a setting a command *does* declare
    // (`--offline`) is that command's boolean flag here.
    let original = ["pnpm", "install", "--shamefully-hoist=false"];
    assert_eq!(resolve(&original), original);
    assert_eq!(
        resolve(&["pnpm", "install", "--offline=false"]),
        ["pnpm", "install", "--no-offline"],
    );
}

#[test]
fn a_value_taking_option_keeps_its_value() {
    for name in ["filter", "dir", "reporter", "store-dir"] {
        let flag = format!("--{name}=false");
        assert_eq!(
            resolve(&["pnpm", "install", &flag]),
            ["pnpm", "install", flag.as_str()],
            "--{name} takes a value of its own",
        );
    }
}

/// Every spelling the pass can emit has to be one the parse accepts, or a
/// resolved value would abort the parse it was meant to fix.
#[test]
fn every_opposite_names_a_real_flag() {
    let command = with_boolean_negations(CliArgs::command());
    let known = |long: &str| {
        let declares =
            |cmd: &clap::Command| cmd.get_arguments().flat_map(spellings).any(|name| name == long);
        declares(&command) || command.get_subcommands().any(declares)
    };
    for (name, opposite) in &boolean_flags().opposites {
        assert!(known(name), "--{name} is no flag");
        if let Some(opposite) = opposite {
            assert!(known(opposite), "--{opposite}, the opposite of --{name}, is no flag");
        }
    }
}
