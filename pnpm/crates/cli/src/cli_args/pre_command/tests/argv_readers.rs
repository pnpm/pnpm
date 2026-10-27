//! The entry points reading a command line before dispatch agree on every
//! bootstrap flag.

use super::{apply_cli_layers, pre_command_input, unparsed_switch_input, version_switch_input};
use crate::{
    cli_args::{
        grammar,
        pre_command::{
            EntryPoint, Invocation, input::bootstrap_argv::BootstrapArgv, should_skip_command_name,
        },
    },
    config_overrides::ConfigOverrides,
};
use clap::Arg;
use pnpm_config::Config;
use std::{
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
};

/// The global flags of [`CliArgs`](super::CliArgs) the pre-command pass does not read. They
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

fn sample_value(arg: &Arg) -> String {
    if arg.get_id() == "color" {
        return "never".to_string();
    }
    arg.get_possible_values()
        .into_iter()
        .rfind(|value| !value.is_hide_set())
        .map_or_else(|| format!("/tmp/{}", arg.get_id()), |value| value.get_name().to_string())
}

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

fn assert_readers_agree(flags: &[String]) {
    let line = |head: &[&str], tail: &[&str]| {
        std::iter::once("pnpm")
            .chain(head.iter().copied())
            .chain(flags.iter().map(String::as_str))
            .chain(tail.iter().copied())
            .map(OsString::from)
            .collect::<Vec<_>>()
    };
    let parsed = Invocation::from_argv(&line(&["install"], &[]), EntryPoint::Command);

    let version = Invocation::from_argv(&line(&[], &["install"]), EntryPoint::Version);
    assert_same_bootstrap_fields(&version, &parsed, &format!("--version scan of {flags:?}"));

    for (head, tail) in
        [(&["install"][..], &["--undeclared"][..]), (&[], &["install", "--undeclared"])]
    {
        let unparsed = Invocation::from_argv(&line(head, tail), EntryPoint::UnknownFlag);
        assert!(!unparsed.ambiguous, "the command name of {flags:?} is unambiguous");
        assert_same_bootstrap_fields(
            &unparsed,
            &parsed,
            &format!("unparsed scan of {head:?} {flags:?} {tail:?}"),
        );
    }
}

fn assert_same_bootstrap_fields(actual: &Invocation, expected: &Invocation, context: &str) {
    assert_eq!(actual.paths, expected.paths, "{context}");
    assert_eq!(actual.command, expected.command, "{context}");
    assert_eq!(actual.frozen_lockfile, expected.frozen_lockfile, "{context}");
    assert_eq!(actual.lockfile_dir, expected.lockfile_dir, "{context}");
    assert_eq!(actual.offline, expected.offline, "{context}");
    assert_eq!(actual.prefer_offline, expected.prefer_offline, "{context}");
    assert_eq!(actual.color, expected.color, "{context}");
    assert_eq!(actual.no_color, expected.no_color, "{context}");
    assert_eq!(actual.network, expected.network, "{context}");
    assert_eq!(actual.reporter, expected.reporter, "{context}");
}

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
        for invocation in [
            Invocation::from_argv(&argv, EntryPoint::Version),
            Invocation::from_argv(&argv, EntryPoint::UnknownFlag),
        ] {
            assert!(!invocation.ambiguous, "{rejected}");
            assert_eq!(
                invocation.paths.dir,
                std::path::PathBuf::from("/tmp/project"),
                "{rejected}",
            );
            assert_eq!(
                invocation.network.registry.as_deref(),
                Some("https://mirror.example.com/"),
                "{rejected}",
            );
        }
    }
}

#[test]
fn version_argv_reads_dir_auth_file_and_command_forms() {
    struct Case {
        name: &'static str,
        argv: &'static [&'static str],
        dir: Option<&'static str>,
        npmrc_auth_file: Option<&'static str>,
        command: Option<&'static str>,
    }

    let cases = [
        Case {
            name: "separate long dir and equals auth file",
            argv: &["pnpm", "--dir", "/tmp/project", "--npmrc-auth-file=auth.ini", "--version"],
            dir: Some("/tmp/project"),
            npmrc_auth_file: Some("auth.ini"),
            command: None,
        },
        Case {
            name: "short dir",
            argv: &["pnpm", "-C", "/tmp/short-dir", "--version"],
            dir: Some("/tmp/short-dir"),
            npmrc_auth_file: None,
            command: None,
        },
        Case {
            name: "equals dir and userconfig alias",
            argv: &["pnpm", "--dir=/tmp/equals-dir", "--userconfig", "user.ini", "--version"],
            dir: Some("/tmp/equals-dir"),
            npmrc_auth_file: Some("user.ini"),
            command: None,
        },
        Case {
            name: "prefix alias of dir",
            argv: &["pnpm", "--prefix", "/tmp/prefix-dir", "--version"],
            dir: Some("/tmp/prefix-dir"),
            npmrc_auth_file: None,
            command: None,
        },
        Case {
            name: "equals prefix alias of dir",
            argv: &["pnpm", "--prefix=/tmp/equals-prefix", "--version"],
            dir: Some("/tmp/equals-prefix"),
            npmrc_auth_file: None,
            command: None,
        },
        Case {
            name: "separator stops command detection",
            argv: &["pnpm", "--dir=/tmp/separator", "--", "run"],
            dir: Some("/tmp/separator"),
            npmrc_auth_file: None,
            command: None,
        },
        Case {
            name: "value-taking global option is skipped",
            argv: &["pnpm", "--filter", "pkg", "--reporter", "append-only", "install"],
            dir: None,
            npmrc_auth_file: None,
            command: Some("install"),
        },
        Case {
            name: "store directory value is not mistaken for the command",
            argv: &["pnpm", "--store", "/tmp/store", "--prefix", "/tmp/scanned", "--version"],
            dir: Some("/tmp/scanned"),
            npmrc_auth_file: None,
            command: None,
        },
        Case {
            name: "canonical store directory value is not mistaken for the command",
            argv: &["pnpm", "--store-dir", "/tmp/store", "--dir", "/tmp/scanned", "--version"],
            dir: Some("/tmp/scanned"),
            npmrc_auth_file: None,
            command: None,
        },
    ];

    for case in cases {
        let argv = case.argv
            .iter()
            .copied()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let input = version_switch_input(&argv);

        if let Some(dir) = case.dir {
            assert_eq!(input.paths.dir, PathBuf::from(dir), "case: {}", case.name);
        }
        assert_eq!(
            input.paths.npmrc_auth_file,
            case.npmrc_auth_file.map(PathBuf::from),
            "case: {}",
            case.name,
        );
        assert_eq!(input.command.as_deref(), case.command, "case: {}", case.name);
    }

    let input = version_switch_input(&[
        OsString::from("pnpm"),
        OsString::from("--state-dir"),
        OsString::from("/tmp/state"),
        OsString::from("--version"),
    ]);
    assert_eq!(input.paths.state_dir.as_deref(), Some(Path::new("/tmp/state")));

    for spelling in ["--store-dir", "--store"] {
        let input = version_switch_input(&[
            OsString::from("pnpm"),
            OsString::from(spelling),
            OsString::from("/tmp/store"),
            OsString::from("--version"),
        ]);
        assert_eq!(
            input.paths.store_dir.as_deref(),
            Some(Path::new("/tmp/store")),
            "spelling: {spelling}",
        );
    }
}

#[test]
fn unparsed_argv_reads_flags_on_both_sides_of_the_command() {
    let argv = [
        "pnpm",
        "--store-dir",
        "/tmp/store",
        "install",
        "--auto-dedupe",
        "--dir",
        "/tmp/project",
        "--frozen-lockfile",
        "--lockfile-dir=/tmp/lockfile",
        "--offline",
        "--registry",
        "https://mirror.example.com/",
        "--no-proxy=internal.example.com",
        "-g",
    ]
    .map(OsString::from);
    let input = unparsed_switch_input(&argv).expect("the command name is unambiguous");
    assert_eq!(input.command.as_deref(), Some("install"));
    assert_eq!(input.paths.dir, PathBuf::from("/tmp/project"));
    assert_eq!(input.paths.store_dir.as_deref(), Some(Path::new("/tmp/store")));
    assert_eq!(input.frozen_lockfile, Some(true));
    assert_eq!(input.lockfile_dir, Some(PathBuf::from("/tmp/lockfile")));
    assert_eq!(input.offline, Some(true));
    assert_eq!(input.prefer_offline, None);
    assert_eq!(input.network.registry.as_deref(), Some("https://mirror.example.com/"));
    assert_eq!(input.network.no_proxy.as_deref(), Some("internal.example.com"));
    assert!(input.global);

    let version_input = version_switch_input(&argv);
    assert_eq!(version_input.paths.dir, PathBuf::from("/tmp/project"));

    let argv = ["pnpm", "exec", "--", "tool", "-g"].map(OsString::from);
    assert!(!unparsed_switch_input(&argv).expect("the command name is unambiguous").global);
}

#[test]
fn unparsed_argv_names_no_command_after_an_undeclared_option() {
    for command in ["config", "get", "set", "install"] {
        let argv =
            ["pnpm", "--undeclared", "value", command, "--also-undeclared"].map(OsString::from);
        assert!(unparsed_switch_input(&argv).is_none(), "{command}");
    }

    let argv = ["pnpm", "--no-color", "--filter", "pkg", "install", "--undeclared", "value"].map(
        OsString::from,
    );
    let input = unparsed_switch_input(&argv).expect("every option before it is declared");
    assert_eq!(input.command.as_deref(), Some("install"));
}

#[test]
fn unparsed_argv_reads_an_undeclared_option_right_before_the_command_as_a_flag() {
    let argv = ["pnpm", "--undeclared", "install", "--also-undeclared"].map(OsString::from);
    let input = unparsed_switch_input(&argv).expect("the command follows the option");
    assert_eq!(input.command.as_deref(), Some("install"));
}

#[test]
fn unparsed_ci_is_a_frozen_install() {
    for argv in [
        &["pnpm", "ci", "--undeclared"][..],
        &["pnpm", "ci", "--no-frozen-lockfile", "--undeclared"],
    ] {
        let argv = argv
            .iter()
            .copied()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let input = unparsed_switch_input(&argv).expect("the command name is unambiguous");
        assert_eq!(input.frozen_lockfile, Some(true), "{argv:?}");
    }
}

#[test]
fn unparsed_argv_steps_over_short_option_values() {
    for (argv, dir) in [
        (&["pnpm", "-C/tmp/attached", "install", "--undeclared"][..], "/tmp/attached"),
        (&["pnpm", "-C", "/tmp/separate", "install", "--undeclared"], "/tmp/separate"),
        (&["pnpm", "-rC", "/tmp/clustered", "install", "--undeclared"], "/tmp/clustered"),
        (&["pnpm", "-rC/tmp/attached-cluster", "install", "--undeclared"], "/tmp/attached-cluster"),
        (&["pnpm", r"-rCC:\project", "install", "--undeclared"], r"C:\project"),
    ] {
        let argv = argv
            .iter()
            .copied()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let input = unparsed_switch_input(&argv).expect("the command name is unambiguous");
        assert_eq!(input.command.as_deref(), Some("install"), "{argv:?}");
        assert_eq!(input.paths.dir, PathBuf::from(dir), "{argv:?}");
    }
}

#[test]
fn the_version_scan_names_a_command_by_its_own_name() {
    let input = version_switch_input(&["pnpm", "ic", "--version"].map(OsString::from));
    assert_eq!(input.command.as_deref(), Some("ci"));
    assert_eq!(input.frozen_lockfile, Some(true));
}

#[test]
fn unparsed_argv_names_a_command_by_its_own_name() {
    for (alias, command) in
        [("c", "config"), ("ic", "ci"), ("clean-install", "ci"), ("rt", "runtime")]
    {
        let input = unparsed_switch_input(&["pnpm", alias, "--undeclared"].map(OsString::from))
            .expect("the command name is unambiguous");
        assert_eq!(input.command.as_deref(), Some(command), "{alias}");
    }
    let input =
        unparsed_switch_input(&["pnpm", "install-clean", "--undeclared"].map(OsString::from))
            .expect("the command name is unambiguous");
    assert_eq!(input.frozen_lockfile, Some(true));
}

#[test]
fn unparsed_argv_reads_global_in_a_short_cluster() {
    for (argv, expected) in [
        (&["pnpm", "add", "-gE", "pkg", "--undeclared"][..], true),
        (&["pnpm", "add", "-Eg", "pkg", "--undeclared"], true),
        (&["pnpm", "-Cg", "add", "pkg", "--undeclared"], false),
        (&["pnpm", "add", "pkg", "--undeclared"], false),
        (&["pnpm", "add", "--filter", "-g", "pkg", "--undeclared"], false),
    ] {
        let argv = argv
            .iter()
            .copied()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let scan = unparsed_switch_input(&argv).expect("the command line is unambiguous");
        assert_eq!(scan.global, expected, "{argv:?}");
    }

    let argv = ["pnpm", "install", "--init-type", "--global", "--undeclared"].map(OsString::from);
    assert!(unparsed_switch_input(&argv).is_none());

    let argv = ["pnpm", "add", "pkg", "--undeclared", "-gE"].map(OsString::from);
    assert!(unparsed_switch_input(&argv).is_none());

    let argv = ["pnpm", "install", "-rF", "-g", "--undeclared"].map(OsString::from);
    let scan = unparsed_switch_input(&argv).expect("the command line is unambiguous");
    assert!(!scan.global);
}

#[test]
fn unparsed_argv_reads_the_project_location() {
    for (argv, expected) in [
        (&["pnpm", "config", "set", "key", "value", "--location", "project"][..], true),
        (&["pnpm", "config", "--location=project", "set", "key", "value"][..], true),
        (&["pnpm", "config", "--location", "global", "set", "key", "value"][..], false),
        (&["pnpm", "config", "set", "key", "value", "--", "--location=project"][..], false),
        (&["pnpm", "config", "list", "--undeclared=value", "--location", "project"][..], true),
    ] {
        let argv = argv
            .iter()
            .copied()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let scan = unparsed_switch_input(&argv).expect("the command line is unambiguous");
        assert_eq!(scan.project_location, expected, "{argv:?}");
    }

    let argv =
        ["pnpm", "config", "list", "--undeclared", "--location", "project"].map(OsString::from);
    assert!(unparsed_switch_input(&argv).is_none());
}

#[test]
fn the_version_scan_reads_ignore_workspace_from_the_command_line() {
    let flag_of = |argv: &[&str]| {
        version_switch_input(
            &argv
                .iter()
                .copied()
                .map(OsString::from)
                .collect::<Vec<_>>(),
        )
        .paths
        .ignore_workspace
    };
    assert!(flag_of(&["pnpm", "--ignore-workspace", "--version"]));
    assert!(!flag_of(&["pnpm", "--no-ignore-workspace", "--version"]));
    assert!(!flag_of(&["pnpm", "--version"]));
    assert!(
        flag_of(&["pnpm", "--dir", "/tmp/scanned", "--ignore-workspace", "--version"]),
        "a preceding value-taking flag must not swallow it",
    );
}

#[test]
fn the_switch_reads_frozen_lockfile_from_the_command_line() {
    let flag_of = |argv: &[&str]| {
        let argv = argv
            .iter()
            .copied()
            .map(OsString::from)
            .collect::<Vec<_>>();
        Invocation::from_argv(&argv, EntryPoint::Command).frozen_lockfile
    };

    assert_eq!(flag_of(&["pnpm", "install", "--frozen-lockfile"]), Some(true));
    assert_eq!(flag_of(&["pnpm", "install", "--no-frozen-lockfile"]), Some(false));
    assert_eq!(flag_of(&["pnpm", "install"]), None);
    assert_eq!(flag_of(&["pnpm", "install-test", "--frozen-lockfile"]), Some(true));
    assert_eq!(flag_of(&["pnpm", "ci"]), Some(true));
    assert_eq!(flag_of(&["pnpm", "run", "build"]), None);
}

#[test]
fn pin_flags_cover_every_command_declaring_them() {
    for subcommand in grammar().get_subcommands() {
        let name = subcommand.get_name();
        if should_skip_command_name(name) {
            continue;
        }
        let declares = |long: &str| {
            subcommand
                .get_arguments()
                .any(|arg| arg.get_long() == Some(long))
        };
        let parse_inv = |extra: &[&str]| {
            let mut argv = vec![OsString::from("pnpm"), OsString::from(name)];
            argv.extend(
                extra
                    .iter()
                    .copied()
                    .map(OsString::from),
            );
            Invocation::from_argv(&argv, EntryPoint::Command)
        };
        if declares("lockfile-dir") {
            let inv = parse_inv(&["--lockfile-dir", "lf"]);
            assert_eq!(
                inv.lockfile_dir.as_deref(),
                Some(Path::new("lf")),
                "`pnpm {name}` accepts --lockfile-dir",
            );
        }
        if declares("offline") {
            let inv = parse_inv(&["--offline"]);
            assert_eq!(inv.offline, Some(true), "`pnpm {name}` accepts --offline");
        }
        if declares("prefer-offline") {
            let inv = parse_inv(&["--prefer-offline"]);
            assert_eq!(inv.prefer_offline, Some(true), "`pnpm {name}` accepts --prefer-offline");
        }
    }
}

#[test]
fn a_negated_flag_clears_a_configured_value() {
    let cases = [
        (None, true, true),
        (None, false, false),
        (Some(false), true, false),
        (Some(true), false, true),
    ];
    for (offline, configured, expected) in cases {
        let mut config = Config { offline: configured, ..Config::default() };
        let inv = Invocation { offline, ..pre_command_input(Path::new("/tmp")) };
        apply_cli_layers(&mut config, &inv, &ConfigOverrides::default(), Path::new("/tmp"))
            .expect("apply cli layers");
        assert_eq!(config.offline, expected, "offline {offline:?} over a configured {configured}");
    }
}
