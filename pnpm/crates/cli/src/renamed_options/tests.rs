use super::drop_shadowed_aliases;
use crate::{
    boolean_negations::with_boolean_negations, cli_args::CliArgs,
    flag_relocation::relocate_pre_subcommand_flags, shorthands::expand_universal_shorthands,
};
use clap::{CommandFactory, FromArgMatches};
use pretty_assertions::assert_eq;
use std::{ffi::OsString, path::Path};

fn drop_aliases(tokens: &[&str]) -> Vec<String> {
    let cmd = with_boolean_negations(CliArgs::command());
    drop_shadowed_aliases(&cmd, tokens.iter().map(OsString::from).collect())
        .into_iter()
        .map(|token| token.into_string().expect("test tokens are UTF-8"))
        .collect()
}

/// Run the full pre-parse pipeline and parse.
fn parse(tokens: &[&str]) -> CliArgs {
    let cmd = with_boolean_negations(CliArgs::command());
    let argv = expand_universal_shorthands(&cmd, tokens.iter().map(OsString::from).collect());
    let argv = drop_shadowed_aliases(&cmd, argv);
    let argv = relocate_pre_subcommand_flags(&cmd, argv);
    cmd.try_get_matches_from(argv)
        .and_then(|matches| CliArgs::from_arg_matches(&matches))
        .expect("parses after the pre-parse pipeline")
}

#[test]
fn a_lone_alias_is_kept() {
    assert_eq!(
        drop_aliases(&["pnpm", "--prefix", "here", "install"]),
        ["pnpm", "--prefix", "here", "install"],
    );
    assert_eq!(
        drop_aliases(&["pnpm", "--store=here", "install"]),
        ["pnpm", "--store=here", "install"],
    );
}

#[test]
fn the_canonical_spelling_wins_over_the_alias_in_either_order() {
    for argv in [
        ["pnpm", "--prefix", "aliased", "--dir", "canonical", "install"].as_slice(),
        ["pnpm", "--dir", "canonical", "--prefix", "aliased", "install"].as_slice(),
        ["pnpm", "--prefix=aliased", "-C", "canonical", "install"].as_slice(),
    ] {
        assert_eq!(parse(argv).dir, Path::new("canonical"), "argv: {argv:?}");
    }

    for argv in [
        ["pnpm", "--store", "aliased", "--store-dir", "canonical", "install"].as_slice(),
        ["pnpm", "--store-dir=canonical", "--store=aliased", "install"].as_slice(),
    ] {
        assert_eq!(
            parse(argv).store_dir.as_deref(),
            Some(Path::new("canonical")),
            "argv: {argv:?}",
        );
    }
}

/// A canonical short option counts only where a short option can appear:
/// the `C` inside a `--filter` pattern attached to `-F` is part of that
/// pattern, not a `--dir`.
#[test]
fn a_canonical_short_inside_an_attached_value_is_not_the_option() {
    assert_eq!(
        drop_aliases(&["pnpm", "-FpkgC", "--prefix", "here", "install"]),
        ["pnpm", "-FpkgC", "--prefix", "here", "install"],
    );
    assert_eq!(parse(&["pnpm", "-FpkgC", "--prefix", "here", "install"]).dir, Path::new("here"));
    // Up to that point the cluster's own options are read: `-r` takes no
    // value, so the `-C` behind it is an option and shadows the alias.
    assert_eq!(
        drop_aliases(&["pnpm", "-rCcanonical", "--prefix", "here", "install"]),
        ["pnpm", "-rCcanonical", "install"],
    );
    assert_eq!(
        parse(&["pnpm", "-rCcanonical", "--prefix", "here", "install"]).dir,
        Path::new("canonical"),
    );
}

/// A cluster ending in a value-taking short takes the next token as its
/// value, so a directory that spells an alias is not read as one.
#[test]
fn a_cluster_s_separate_value_is_not_read_as_an_option() {
    assert_eq!(
        drop_aliases(&["pnpm", "-rC", "--prefix", "install"]),
        ["pnpm", "-rC", "--prefix", "install"],
    );
}

/// Only pnpm's own tokens are considered: a script's `--prefix` is the
/// script's, and so is an option value that happens to spell one.
#[test]
fn forwarded_and_value_tokens_are_left_alone() {
    assert_eq!(
        drop_aliases(&["pnpm", "--dir", "here", "run", "build", "--prefix", "there"]),
        ["pnpm", "--dir", "here", "run", "build", "--prefix", "there"],
    );
    assert_eq!(
        drop_aliases(&["pnpm", "--dir", "here", "--", "--prefix", "there"]),
        ["pnpm", "--dir", "here", "--", "--prefix", "there"],
    );
    assert_eq!(
        drop_aliases(&["pnpm", "--dir", "here", "--filter", "--prefix", "install"]),
        ["pnpm", "--dir", "here", "--filter", "--prefix", "install"],
    );
}
