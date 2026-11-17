use clap::Parser;
use pretty_assertions::assert_eq;

use super::DependencySpecifiers;
use crate::cli_args::{CliArgs, cli_command::CliCommand};

fn dependency_specifiers(argv: &[&str]) -> DependencySpecifiers {
    match CliArgs::try_parse_from(argv).expect("parses").command {
        CliCommand::Why(why) => why.dependency_specifiers(),
        other => panic!("expected why, got {other:?}"),
    }
}

#[test]
fn reads_the_specifiers_the_selected_format_shows() {
    for (argv, expected) in [
        (&["pnpm", "why", "foo"][..], DependencySpecifiers::Omit),
        (&["pnpm", "why", "--parseable", "foo"], DependencySpecifiers::Omit),
        (&["pnpm", "why", "--json", "foo"], DependencySpecifiers::All),
        (&["pnpm", "why", "--json", "--parseable", "foo"], DependencySpecifiers::Omit),
        (&["pnpm", "why", "--ranges", "foo"], DependencySpecifiers::DirectDependents),
    ] {
        assert_eq!(dependency_specifiers(argv), expected, "{argv:?}");
    }
}
