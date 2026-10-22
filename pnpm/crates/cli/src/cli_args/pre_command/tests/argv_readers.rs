//! The three ways the pre-command pass reads a command line agree.

use super::{CliArgs, SwitchInput};
use crate::{
    boolean_negations::with_boolean_negations,
    cli_args::{
        grammar,
        pre_command::input::{UnparsedArgv, VersionArgv, bootstrap_argv::BootstrapArgv},
        reporter::ReporterFlags,
    },
};
use clap::{Arg, CommandFactory};
use std::{collections::HashSet, ffi::OsString};

/// The global flags of [`CliArgs`] the pre-command pass does not read. They
/// shape the command itself, which the pnpm the project pins runs.
const READ_AFTER_THE_SWITCH: &[&str] = &[
    "aggregate_output",
    "bail",
    "changed_files_ignore_pattern",
    "env_file",
    "fail_if_no_match",
    "filter",
    "filter_prod",
    "include_workspace_root",
    "no_bail",
    "no_fail_if_no_match",
    "no_include_workspace_root",
    "no_reporter_hide_prefix",
    "no_reverse",
    "no_sort",
    "parallel",
    "recursive",
    "report_summary",
    "reporter_hide_prefix",
    "resume_from",
    "reverse",
    "sort",
    "stream",
    "test_pattern",
    "use_stderr",
    "workspace_concurrency",
    "workspace_root",
    "yes",
];

#[test]
fn every_global_flag_is_read_before_the_switch_or_after_it() {
    let bootstrap = BootstrapArgv::command()
        .get_arguments()
        .map(|arg| arg.get_id().as_str())
        .collect::<HashSet<_>>();
    let global = grammar()
        .get_arguments()
        .filter(|arg| arg.is_global_set())
        .map(|arg| arg.get_id().as_str())
        .collect::<Vec<_>>();

    let unclassified = global
        .iter()
        .filter(|id| !bootstrap.contains(*id) && !READ_AFTER_THE_SWITCH.contains(id))
        .collect::<Vec<_>>();
    assert!(
        unclassified.is_empty(),
        "add these global flags to a group `BootstrapArgv` flattens, or to READ_AFTER_THE_SWITCH: {unclassified:?}",
    );
    let both = READ_AFTER_THE_SWITCH
        .iter()
        .filter(|id| bootstrap.contains(*id))
        .collect::<Vec<_>>();
    assert!(both.is_empty(), "read before the switch, so not after it: {both:?}");
    let stale = READ_AFTER_THE_SWITCH
        .iter()
        .filter(|id| !global.contains(id))
        .collect::<Vec<_>>();
    assert!(stale.is_empty(), "no longer global flags of `pnpm`: {stale:?}");
}

#[test]
fn every_reader_reads_each_bootstrap_flag_the_same_way() {
    let mut first_spellings = Vec::new();
    for arg in BootstrapArgv::command().get_arguments() {
        let spellings = spellings(arg);
        for spelling in &spellings {
            assert_readers_agree(&with_dir(arg, spelling));
        }
        first_spellings.extend(spellings.into_iter().next());
    }
    let every_flag = first_spellings
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    assert_readers_agree(&every_flag);
}

/// Every way to type `arg`: each long name and alias, with the value
/// attached or as the next token, and each short name.
fn spellings(arg: &Arg) -> Vec<Vec<String>> {
    let longs = arg
        .get_long()
        .into_iter()
        .chain(
            arg.get_all_aliases()
                .into_iter()
                .flatten(),
        );
    let shorts = arg
        .get_short()
        .into_iter()
        .chain(
            arg.get_all_short_aliases()
                .into_iter()
                .flatten(),
        );
    let takes_value = arg.get_action().takes_values()
        && arg.get_num_args().is_none_or(|range| range.takes_values());
    if !takes_value {
        return longs
            .map(|long| vec![format!("--{long}")])
            .chain(shorts.map(|short| vec![format!("-{short}")]))
            .collect();
    }
    let value = sample_value(arg);
    let mut spellings = Vec::new();
    for long in longs {
        spellings.push(vec![format!("--{long}={value}")]);
        if arg.is_require_equals_set() {
            if arg
                .get_num_args()
                .is_some_and(|range| range.min_values() == 0)
            {
                spellings.push(vec![format!("--{long}")]);
            }
        } else {
            spellings.push(vec![format!("--{long}"), value.clone()]);
        }
    }
    for short in shorts {
        spellings.push(vec![format!("-{short}"), value.clone()]);
        spellings.push(vec![format!("-{short}{value}")]);
    }
    spellings
}

/// A value `arg` accepts. A value parser written as a function lists no
/// possible values to clap, so its flags name one here.
fn sample_value(arg: &Arg) -> String {
    if arg.get_id() == "color" {
        return "never".to_string();
    }
    arg.get_possible_values()
        .into_iter()
        .rfind(|value| !value.is_hide_set())
        .map_or_else(|| format!("/tmp/{}", arg.get_id()), |value| value.get_name().to_string())
}

/// `spelling`, after a `--dir` unless it spells `--dir` itself: without
/// one, the parsed command line keeps clap's default where the scans
/// resolve the local prefix.
fn with_dir(arg: &Arg, spelling: &[String]) -> Vec<String> {
    if arg.get_id() == "dir" {
        return spelling.to_vec();
    }
    ["--dir", "/tmp/project"]
        .map(String::from)
        .into_iter()
        .chain(spelling.iter().cloned())
        .collect()
}

/// The parsed command line, the `--version` scan, and the scan of a command
/// line clap rejected, with the flags on either side of the command name,
/// all read `flags` into the same input.
fn assert_readers_agree(flags: &[String]) {
    let line = |head: &[&str], tail: &[&str]| {
        std::iter::once("pnpm")
            .chain(head.iter().copied())
            .chain(flags.iter().map(String::as_str))
            .chain(tail.iter().copied())
            .map(OsString::from)
            .collect::<Vec<_>>()
    };
    let parsed = parse(&line(&["install"], &[]));

    let version = VersionArgv::scan(&line(&[], &["install"]));
    assert_eq!((version.switch, version.reporter), parsed, "--version scan of {flags:?}");

    for (head, tail) in
        [(&["install"][..], &["--undeclared"][..]), (&[], &["install", "--undeclared"])]
    {
        let unparsed = UnparsedArgv::scan(&line(head, tail))
            .unwrap_or_else(|| panic!("the command name of {flags:?} is unambiguous"));
        assert_eq!(
            (unparsed.switch, unparsed.reporter),
            parsed,
            "unparsed scan of {head:?} {flags:?} {tail:?}",
        );
    }
}

fn parse(argv: &[OsString]) -> (SwitchInput, ReporterFlags) {
    let args = crate::parse_cli_args(with_boolean_negations(CliArgs::command()), argv.to_vec())
        .unwrap_or_else(|error| panic!("parse {argv:?}: {error}"));
    (SwitchInput::from_cli_args(&args), args.reporter_flags())
}

/// A scanned command line is one this pnpm could not parse, so a value it
/// rejects must not cost the flags around it.
#[test]
fn a_rejected_value_leaves_the_other_flags_read() {
    for rejected in ["--reporter=bogus", "--color=bogus", "--loglevel=bogus"] {
        let argv = [
            "pnpm",
            "--dir",
            "/tmp/project",
            rejected,
            "--registry=https://mirror.example.com/",
            "install",
            "--undeclared",
        ]
        .map(OsString::from);
        for switch in [
            VersionArgv::scan(&argv).switch,
            UnparsedArgv::scan(&argv).expect("the command name is unambiguous").switch,
        ] {
            assert_eq!(switch.paths.dir, std::path::PathBuf::from("/tmp/project"), "{rejected}");
            assert_eq!(
                switch.network.registry.as_deref(),
                Some("https://mirror.example.com/"),
                "{rejected}",
            );
        }
    }
}
