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
