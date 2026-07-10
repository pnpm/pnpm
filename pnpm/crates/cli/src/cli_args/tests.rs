use super::{
    CliArgs,
    cli_command::CliCommand,
    install::{InstallArgs, resolve_bool_override},
    list::RecursionLimit,
    package_manager::{
        current_source_pnpm_version, package_manager_to_sync, parse_package_manager,
    },
};
use clap::Parser;
use pacquet_default_reporter::SummaryScope;
use std::path::Path;
use tempfile::TempDir;

fn install_args(argv: &[&str]) -> InstallArgs {
    match CliArgs::try_parse_from(argv).expect("parses").command {
        CliCommand::Install(install) => install,
        other => panic!("expected install, got {other:?}"),
    }
}

fn default_reporter_summary_scope(argv: &[&str]) -> SummaryScope {
    CliArgs::try_parse_from(argv).expect("parses").command.default_reporter_summary_scope()
}

#[test]
fn store_dir_is_global_and_parses_on_either_side_of_the_subcommand() {
    for argv in [
        ["pacquet", "--store-dir", "custom-store", "install"].as_slice(),
        ["pacquet", "install", "--store-dir=custom-store"].as_slice(),
    ] {
        let parsed = CliArgs::try_parse_from(argv).expect("parses global --store-dir");
        assert_eq!(parsed.store_dir.as_deref(), Some(Path::new("custom-store")));
    }
}

#[test]
fn store_dir_accepts_an_explicit_empty_value() {
    let parsed = CliArgs::try_parse_from(["pacquet", "store", "path", "--store-dir="])
        .expect("parses empty global --store-dir");
    assert_eq!(parsed.store_dir.as_deref(), Some(Path::new("")));
}

#[test]
fn recursive_default_is_false() {
    let parsed = CliArgs::try_parse_from(["pacquet", "install"]).expect("parses");
    assert!(!parsed.recursive, "flag absent → false");
}

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

#[test]
fn filter_defaults_are_empty() {
    let parsed = CliArgs::try_parse_from(["pacquet", "install"]).expect("parses");
    assert!(parsed.filter.is_empty(), "no `--filter` → empty");
    assert!(parsed.filter_prod.is_empty(), "no `--filter-prod` → empty");
}

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

#[test]
fn filter_flag_is_global_and_parses_before_subcommand() {
    let parsed = CliArgs::try_parse_from(["pacquet", "-F", "@scope/*", "install"])
        .expect("parses -F install");
    assert_eq!(parsed.filter, ["@scope/*"]);
    assert!(matches!(parsed.command, CliCommand::Install(_)));
}

#[test]
fn recursive_run_flags_parse_before_fallback_command() {
    let parsed = CliArgs::try_parse_from([
        "pacquet",
        "--no-sort",
        "--workspace-concurrency=1",
        "-r",
        "--report-summary",
        ".test",
    ])
    .expect("parses recursive fallback flags");
    assert!(parsed.recursive);
    assert!(parsed.no_sort);
    assert_eq!(parsed.workspace_concurrency, Some(1));
    assert!(parsed.report_summary);
    assert!(
        matches!(&parsed.command, CliCommand::External(command) if command.as_slice() == [".test"]),
    );
    parsed.validate_command_scoped_global_options().expect("recursive fallback flags are valid");
}

#[test]
fn script_scoped_global_flags_parse_before_script_commands() {
    for argv in [
        ["pacquet", "--report-summary", "run", "build"].as_slice(),
        ["pacquet", "--resume-from", "pkg", "exec", "echo"].as_slice(),
        ["pacquet", "--no-bail", "run", "build"].as_slice(),
        ["pacquet", "--report-summary", "test"].as_slice(),
        ["pacquet", "--resume-from", "pkg", "start"].as_slice(),
        ["pacquet", "--no-bail", "stop"].as_slice(),
        ["pacquet", "-r", "--report-summary", ".test"].as_slice(),
    ] {
        let parsed = CliArgs::try_parse_from(argv).expect("parses script-scoped global flag");
        parsed.validate_command_scoped_global_options().expect("script command accepts flag");
    }
}

#[test]
fn if_present_flag_parses_before_script_commands() {
    for argv in [
        ["pacquet", "--if-present", "run", "build"].as_slice(),
        ["pacquet", "--if-present", "test"].as_slice(),
        ["pacquet", "--if-present", "start"].as_slice(),
        ["pacquet", "--if-present", "stop"].as_slice(),
        ["pacquet", "--if-present", "restart"].as_slice(),
    ] {
        let parsed = CliArgs::try_parse_from(argv).expect("parses top-level --if-present");
        assert!(parsed.if_present);
        parsed.validate_command_scoped_global_options().expect("script command accepts flag");
    }
}

/// The exact shape of the repo's own `test-pkgs-branch` script.
#[test]
fn if_present_flag_parses_before_fallback_command() {
    let parsed = CliArgs::try_parse_from([
        "pacquet",
        "--workspace-concurrency=1",
        "--filter=...[origin/main]",
        "--no-sort",
        "--if-present",
        ".test",
    ])
    .expect("parses top-level --if-present with a fallback script");
    assert!(parsed.if_present);
    assert!(
        matches!(&parsed.command, CliCommand::External(command) if command.as_slice() == [".test"]),
    );
    parsed.validate_command_scoped_global_options().expect("fallback command accepts flag");
}

#[test]
fn if_present_flag_rejects_non_script_commands() {
    for argv in [
        ["pacquet", "--if-present", "install"].as_slice(),
        ["pacquet", "--if-present", "publish"].as_slice(),
        // `exec` accepts the other run-scoped flags but runs arbitrary
        // commands, not scripts — pnpm rejects `--if-present` for it.
        ["pacquet", "--if-present", "exec", "ls"].as_slice(),
    ] {
        let parsed =
            CliArgs::try_parse_from(argv).expect("global parser accepts compatibility flag");
        let err = parsed
            .validate_command_scoped_global_options()
            .expect_err("non-script command rejects flag");
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }
    // Not `global = true` (the script subcommands declare their own
    // `--if-present`), so after a non-script subcommand it fails at
    // parse time instead of validation.
    CliArgs::try_parse_from(["pacquet", "install", "--if-present"])
        .expect_err("install rejects --if-present at parse time");
}

#[test]
fn report_summary_global_flag_parses_for_publish() {
    for argv in [
        ["pacquet", "--report-summary", "publish"].as_slice(),
        ["pacquet", "publish", "--report-summary"].as_slice(),
    ] {
        let parsed = CliArgs::try_parse_from(argv).expect("parses report-summary for publish");
        parsed.validate_command_scoped_global_options().expect("publish accepts report-summary");
    }
}

#[test]
fn script_scoped_global_flags_reject_unrelated_commands() {
    for argv in [
        ["pacquet", "install", "--report-summary"].as_slice(),
        ["pacquet", "install", "--resume-from", "pkg"].as_slice(),
        ["pacquet", "install", "--no-bail"].as_slice(),
        ["pacquet", "restart", "--report-summary"].as_slice(),
        ["pacquet", "restart", "--no-bail"].as_slice(),
        ["pacquet", "publish", "--resume-from", "pkg"].as_slice(),
        ["pacquet", "publish", "--no-bail"].as_slice(),
    ] {
        let parsed =
            CliArgs::try_parse_from(argv).expect("global parser accepts compatibility flag");
        let err = parsed
            .validate_command_scoped_global_options()
            .expect_err("non-script command rejects flag");
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }
}

#[test]
fn recursive_list_accepts_depth_minus_one_as_separate_value() {
    let parsed = CliArgs::try_parse_from(["pacquet", "-r", "list", "--depth", "-1", "--json"])
        .expect("parses recursive list with --depth -1");
    assert!(parsed.recursive);
    let CliCommand::List(args) = parsed.command else {
        panic!("expected list command");
    };
    assert_eq!(args.depth, RecursionLimit::ProjectsOnly);
    assert!(args.json);
}

#[test]
fn workspace_concurrency_parses_as_global_option() {
    let positive = CliArgs::try_parse_from(["pacquet", "--workspace-concurrency", "3", "install"])
        .expect("parses --workspace-concurrency 3");
    assert_eq!(positive.workspace_concurrency, Some(3));

    let negative = CliArgs::try_parse_from(["pacquet", "install", "--workspace-concurrency=-1"])
        .expect("parses --workspace-concurrency=-1 after subcommand");
    assert_eq!(negative.workspace_concurrency, Some(-1));
}

#[test]
fn filter_flag_split_across_subcommand_keeps_only_subcommand_side() {
    let parsed = CliArgs::try_parse_from(["pacquet", "-F", "a", "install", "-F", "b"])
        .expect("parses split -F");
    assert_eq!(parsed.filter, ["b"], "global-side `a` is dropped");
}

#[test]
fn filter_promotes_recursive_without_explicit_flag() {
    let mut parsed =
        CliArgs::try_parse_from(["pacquet", "--filter", "@scope/*", "install"]).expect("parses");
    assert!(!parsed.recursive, "the raw -r flag is absent");
    parsed.promote_recursive_for_filter();
    assert!(parsed.recursive, "a --filter selector promotes to recursive");
}

#[test]
fn filter_prod_promotes_recursive_without_explicit_flag() {
    let mut parsed =
        CliArgs::try_parse_from(["pacquet", "--filter-prod", "app...", "install"]).expect("parses");
    parsed.promote_recursive_for_filter();
    assert!(parsed.recursive, "a --filter-prod selector promotes to recursive");
}

#[test]
fn no_filter_leaves_recursive_untouched() {
    let mut parsed = CliArgs::try_parse_from(["pacquet", "install"]).expect("parses");
    parsed.promote_recursive_for_filter();
    assert!(!parsed.recursive, "without a filter the command stays non-recursive");

    let mut explicit =
        CliArgs::try_parse_from(["pacquet", "-r", "install"]).expect("parses -r install");
    explicit.promote_recursive_for_filter();
    assert!(explicit.recursive, "an explicit -r is preserved");
}

#[test]
fn runtime_alias_and_flags_parse() {
    let parsed = CliArgs::try_parse_from(["pacquet", "rt", "set", "node", "22", "-P"])
        .expect("parses runtime alias");
    let CliCommand::Runtime(args) = parsed.command else {
        panic!("expected runtime command");
    };
    assert!(!args.global);
    assert!(!args.save_dev);
    assert!(args.save_prod);
    assert_eq!(args.params, ["set", "node", "22"]);
}

#[test]
fn runtime_global_flag_parses_after_version() {
    let parsed = CliArgs::try_parse_from(["pacquet", "runtime", "set", "node", "22", "-g"])
        .expect("parses runtime global flag after params");
    let CliCommand::Runtime(args) = parsed.command else {
        panic!("expected runtime command");
    };
    assert!(args.global);
    assert_eq!(args.params, ["set", "node", "22"]);
}

#[test]
fn default_reporter_summary_scope_matches_install_summary_prefixes() {
    assert_eq!(
        default_reporter_summary_scope(&["pacquet", "add", "foo", "-g"]),
        SummaryScope::AllPrefixes,
    );
    assert_eq!(
        default_reporter_summary_scope(&["pacquet", "update", "-g"]),
        SummaryScope::AllPrefixes,
    );
    assert_eq!(
        default_reporter_summary_scope(&["pacquet", "runtime", "set", "node", "22", "-g"]),
        SummaryScope::AllPrefixes,
    );
    assert_eq!(
        default_reporter_summary_scope(&["pacquet", "dlx", "@foo/touch-file-one-bin"]),
        SummaryScope::AllPrefixes,
    );
    assert_eq!(
        default_reporter_summary_scope(&["pacquet", "create", "touch-file-one-bin"]),
        SummaryScope::AllPrefixes,
    );
    assert_eq!(
        default_reporter_summary_scope(&["pacquet", "add", "foo"]),
        SummaryScope::CurrentPrefix,
    );
    assert_eq!(
        default_reporter_summary_scope(&["pacquet", "remove", "foo", "-g"]),
        SummaryScope::AllPrefixes,
    );
}

#[test]
fn link_command_parses_with_name_and_alias() {
    let parsed =
        CliArgs::try_parse_from(["pacquet", "link", "../foo"]).expect("parses pacquet link");
    let CliCommand::Link(args) = &parsed.command else {
        panic!("expected Link command, got {:?}", parsed.command);
    };
    assert_eq!(args.package_paths, ["../foo"]);
}

#[test]
fn link_command_parses_ln_alias() {
    let parsed = CliArgs::try_parse_from(["pacquet", "ln", "../bar"]).expect("parses pacquet ln");
    let CliCommand::Link(args) = &parsed.command else {
        panic!("expected Link command for ln alias, got {:?}", parsed.command);
    };
    assert_eq!(args.package_paths, ["../bar"]);
}

#[test]
fn link_command_parses_multiple_paths() {
    let parsed = CliArgs::try_parse_from(["pacquet", "link", "../a", "../b", "../c"])
        .expect("parses pacquet link with multiple paths");
    let CliCommand::Link(args) = &parsed.command else {
        panic!("expected Link command, got {:?}", parsed.command);
    };
    assert_eq!(args.package_paths, ["../a", "../b", "../c"]);
}

#[test]
fn install_command_parses_i_alias() {
    let parsed = CliArgs::try_parse_from(["pacquet", "i"]).expect("parses pacquet i");
    assert!(
        matches!(parsed.command, CliCommand::Install(_)),
        "`i` is the install alias, got {:?}",
        parsed.command,
    );
}

#[test]
fn unknown_top_level_command_parses_as_external() {
    let parsed = CliArgs::try_parse_from([
        "pacquet",
        "commitlint",
        "--edit",
        "--config=commitlint.config.cjs",
    ])
    .expect("parses external command");
    let CliCommand::External(command) = parsed.command else {
        panic!("expected external command");
    };
    assert_eq!(command, ["commitlint", "--edit", "--config=commitlint.config.cjs"]);
}

#[test]
fn unknown_top_level_command_preserves_global_options() {
    let parsed = CliArgs::try_parse_from(["pacquet", "--dir", "project", "commitlint"])
        .expect("parses external command with globals");
    let CliCommand::External(command) = parsed.command else {
        panic!("expected external command");
    };
    assert_eq!(parsed.dir, std::path::PathBuf::from("project"));
    assert_eq!(command, ["commitlint"]);
}

#[test]
fn parse_package_manager_handles_unscoped_scoped_and_url_references() {
    // Unscoped `name@version`.
    assert_eq!(
        parse_package_manager("pnpm@10.0.0"),
        ("pnpm".to_string(), Some("10.0.0".to_string())),
    );
    // A leading `@` is a scope, so the separator is the *next* `@`.
    assert_eq!(
        parse_package_manager("@scope/pnpm@10.0.0"),
        ("@scope/pnpm".to_string(), Some("10.0.0".to_string())),
    );
    // No `@` separator → bare name, no version.
    assert_eq!(parse_package_manager("pnpm"), ("pnpm".to_string(), None));
    assert_eq!(parse_package_manager("@scope/pnpm"), ("@scope/pnpm".to_string(), None));
    // The integrity hash carried as `+`-suffixed build metadata is dropped.
    assert_eq!(
        parse_package_manager("pnpm@10.0.0+sha512.abc"),
        ("pnpm".to_string(), Some("10.0.0".to_string())),
    );
    // A URL reference (contains `:`) yields no version. Splitting on the first
    // `@` keeps a URL's embedded `@` (e.g. credentials) inside the reference,
    // so the `:` is still seen and the version is correctly dropped.
    assert_eq!(
        parse_package_manager("pnpm@https://user@example.com/pnpm.tgz"),
        ("pnpm".to_string(), None),
    );
}

#[test]
fn package_manager_to_sync_preserves_dev_engine_specifier() {
    let root = TempDir::new().expect("tmp dir");
    let manifest_path = root.path().join("package.json");
    std::fs::write(
        &manifest_path,
        r#"{"devEngines":{"packageManager":{"name":"pnpm","version":">=0.0.0","onFail":"download"}}}"#,
    )
    .expect("write manifest");

    let package_manager = package_manager_to_sync(&manifest_path, root.path())
        .expect("read policy")
        .expect("sync package manager");

    assert_eq!(package_manager.specifier, ">=0.0.0");
    assert_eq!(
        package_manager.version,
        current_source_pnpm_version().expect("source pnpm version"),
    );
}

#[test]
fn resolve_bool_override_tri_state() {
    // force_on wins, force_off wins over a config `true`, and an unset
    // pair falls through to config — in both config polarities.
    assert!(resolve_bool_override(true, false, false), "force_on over config false");
    assert!(resolve_bool_override(true, false, true), "force_on over config true");
    assert!(!resolve_bool_override(false, true, true), "force_off over config true");
    assert!(!resolve_bool_override(false, true, false), "force_off over config false");
    assert!(resolve_bool_override(false, false, true), "unset falls through to config true");
    assert!(!resolve_bool_override(false, false, false), "unset falls through to config false");
}

#[test]
fn trust_lockfile_pair_resolves_last_one_wins() {
    assert!(install_args(&["pacquet", "install", "--no-trust-lockfile"]).no_trust_lockfile);
    assert!(install_args(&["pacquet", "install", "--trust-lockfile"]).trust_lockfile);

    // Both spellings in one argv must not error (pnpm forwards raw tokens);
    // mutual `overrides_with` collapses them to the last-specified.
    let last_off = install_args(&["pacquet", "install", "--trust-lockfile", "--no-trust-lockfile"]);
    assert!(last_off.no_trust_lockfile && !last_off.trust_lockfile, "--no wins when last");
    let last_on = install_args(&["pacquet", "install", "--no-trust-lockfile", "--trust-lockfile"]);
    assert!(last_on.trust_lockfile && !last_on.no_trust_lockfile, "--trust wins when last");
}

#[test]
fn config_merged_boolean_negations_parse() {
    // Each config-OR-merged boolean now exposes an explicit `--no-` inverse
    // so the CLI can force a yaml `true` back off, matching pnpm.
    let args = install_args(&[
        "pacquet",
        "install",
        "--no-offline",
        "--no-prefer-offline",
        "--no-frozen-store",
        "--no-ignore-scripts",
    ]);
    assert!(args.no_offline);
    assert!(args.no_prefer_offline);
    assert!(args.no_frozen_store);
    assert!(args.no_ignore_scripts);
}
