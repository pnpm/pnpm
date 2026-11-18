use crate::cli_args::CliArgs;
use clap::CommandFactory;
use std::collections::{BTreeMap, BTreeSet};

const COMMANDS: &str = include_str!("../../../wasm/webcontainer-commands.tsv");

#[test]
fn every_clap_command_has_a_webcontainer_classification() {
    let rows = COMMANDS
        .lines()
        .skip(1)
        .map(|line| {
            let fields = line.split('\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 3, "invalid command inventory row: {line}");
            assert!(matches!(fields[1], "partial" | "routed" | "none"));
            assert!(
                fields[0].starts_with("mode:") || fields[1] != "none",
                "every CLI command must be exercised by the WebContainer routing audit: {line}"
            );
            assert!(matches!(
                fields[2],
                "available"
                    | "host-limited"
                    | "host-blocked"
                    | "implementation-gap"
                    | "not-implemented"
            ));
            (fields[0].to_owned(), fields[1].to_owned(), fields[2].to_owned())
        })
        .collect::<Vec<_>>();
    let classifications = rows
        .iter()
        .filter(|(path, _, _)| !path.starts_with("mode:"))
        .map(|(path, coverage, capability)| (path.clone(), (coverage.clone(), capability.clone())))
        .collect::<BTreeMap<_, _>>();
    let unique_keys = rows
        .iter()
        .map(|(path, _, _)| path)
        .collect::<BTreeSet<_>>();
    assert_eq!(unique_keys.len(), rows.len(), "inventory keys must be unique");

    let mut actual = BTreeSet::new();
    collect_commands(&CliArgs::command(), "", &mut actual);
    let classified = classifications
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual, classified,
        "update pnpm/wasm/webcontainer-commands.tsv for the Clap command tree",
    );

    for (classification, expected) in [
        ("host-blocked", BTreeSet::from(["pack-app"])),
        ("implementation-gap", BTreeSet::from(["self-update", "setup", "with"])),
        ("host-limited", BTreeSet::from(["docs", "env", "login", "repo", "runtime"])),
        ("not-implemented", BTreeSet::from(["edit", "xmas"])),
    ] {
        assert_eq!(
            classifications
                .iter()
                .filter(|(path, (_, actual))| {
                    !path.starts_with("alias:") && actual == classification
                })
                .map(|(path, _)| path.as_str())
                .collect::<BTreeSet<_>>(),
            expected,
        );
    }

    let modes = rows
        .iter()
        .filter(|(path, _, _)| path.starts_with("mode:"))
        .map(|(path, _, _)| path.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        modes,
        BTreeSet::from([
            "mode:auth-and-registry-operations",
            "mode:cargo-workspace-management",
            "mode:command-overriding-script",
            "mode:custom-network-controls",
            "mode:external-script-dispatch",
            "mode:filter-selection",
            "mode:global-install",
            "mode:interactive-terminal",
            "mode:lifecycle-scripts",
            "mode:metadata-rollback",
            "mode:native-dependency-builds",
            "mode:package-manager-version-switch",
            "mode:permission-inheritance-race",
            "mode:pnpm-pm-dispatch",
            "mode:pnpmfile-hooks",
            "mode:provenance-signing",
            "mode:publish-registry",
            "mode:redirect-authentication",
            "mode:recursive-execution",
            "mode:store-linkers",
            "mode:watch-and-cancellation",
        ]),
    );
}

fn collect_commands(command: &clap::Command, prefix: &str, paths: &mut BTreeSet<String>) {
    for child in command.get_subcommands() {
        let path = if prefix.is_empty() {
            child.get_name().to_owned()
        } else {
            format!("{prefix} {}", child.get_name())
        };
        paths.insert(path.clone());
        for alias in child.get_all_aliases() {
            paths.insert(format!("alias:{path}={alias}"));
        }
        collect_commands(child, &path, paths);
    }
}
