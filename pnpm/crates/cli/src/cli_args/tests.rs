use super::{
    CliArgs,
    add::AddArgs,
    cli_command::{CliCommand, WorkspaceRootError},
    config::{ConfigLocation, ConfigSubcommand},
    dedupe::DedupeArgs,
    install::{InstallArgs, resolve_bool_override},
    list::RecursionLimit,
    package_manager::{
        current_source_pnpm_version, package_manager_to_sync, parse_package_manager,
        read_manifest_json,
    },
    reporter::{LogLevelSetting, ReporterType},
    store::StoreCommand,
    unlink::UnlinkArgs,
};
use clap::Parser;
use pnpm_config::ColorMode;
use pnpm_default_reporter::SummaryScope;
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

fn add_args(argv: &[&str]) -> AddArgs {
    match CliArgs::try_parse_from(argv).expect("parses").command {
        CliCommand::Add(add) => add,
        other => panic!("expected add, got {other:?}"),
    }
}

#[test]
fn dir_is_global_and_parses_on_either_side_of_the_subcommand() {
    for argv in [
        ["pacquet", "--dir", "project", "add", "foo"].as_slice(),
        ["pacquet", "add", "foo", "--dir", "project"].as_slice(),
        ["pacquet", "add", "foo", "-C", "project"].as_slice(),
    ] {
        let parsed = CliArgs::try_parse_from(argv).expect("parses global --dir");
        assert_eq!(parsed.dir, std::path::PathBuf::from("project"));
        assert!(matches!(parsed.command, CliCommand::Add(_)));
    }
}

#[test]
fn prefix_is_an_alias_of_dir() {
    for argv in [
        ["pacquet", "--prefix", "project", "run", "test"].as_slice(),
        ["pacquet", "--prefix=project", "run", "test"].as_slice(),
        ["pacquet", "install", "--prefix", "project"].as_slice(),
    ] {
        let parsed = CliArgs::try_parse_from(argv).expect("parses --prefix");
        assert_eq!(parsed.dir, std::path::PathBuf::from("project"));
    }
}

#[test]
fn registry_is_a_universal_global_option() {
    for argv in [
        ["pacquet", "--registry=https://r.test/", "add", "foo"].as_slice(),
        ["pacquet", "add", "foo", "--registry=https://r.test/"].as_slice(),
        ["pacquet", "view", "foo", "--registry=https://r.test/"].as_slice(),
    ] {
        let parsed = CliArgs::try_parse_from(argv).expect("parses universal --registry");
        assert_eq!(parsed.registry.as_deref(), Some("https://r.test/"));
    }
}

#[test]
fn add_allow_build_collects_repeated_values() {
    let args =
        add_args(&["pacquet", "add", "foo", "--allow-build=esbuild", "--allow-build", "sharp"]);
    assert_eq!(args.allow_build, ["esbuild", "sharp"]);
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
fn store_is_an_alias_of_store_dir() {
    for argv in [
        ["pacquet", "--store", "custom-store", "install"].as_slice(),
        ["pacquet", "install", "--store=custom-store"].as_slice(),
    ] {
        let parsed = CliArgs::try_parse_from(argv).expect("parses --store");
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
fn state_dir_is_global_and_parses_on_either_side_of_the_subcommand() {
    for argv in [
        ["pacquet", "--state-dir", "custom-state", "install"].as_slice(),
        ["pacquet", "install", "--state-dir=custom-state"].as_slice(),
    ] {
        let parsed = CliArgs::try_parse_from(argv).expect("parses global --state-dir");
        assert_eq!(parsed.state_dir.as_deref(), Some(Path::new("custom-state")));
    }
}

#[test]
fn repeated_state_dir_uses_the_last_value_on_either_side_of_the_subcommand() {
    for argv in [
        ["pacquet", "--state-dir", "first-state", "--state-dir", "last-state", "install"]
            .as_slice(),
        ["pacquet", "install", "--state-dir=first-state", "--state-dir=last-state"].as_slice(),
    ] {
        let parsed = CliArgs::try_parse_from(argv).expect("parses repeated global --state-dir");
        assert_eq!(parsed.state_dir.as_deref(), Some(Path::new("last-state")));
    }
}

#[test]
fn proxy_flags_are_global_and_parse_on_either_side_of_the_subcommand() {
    let before =
        CliArgs::try_parse_from(["pacquet", "--https-proxy=http://proxy.example:8443", "install"])
            .expect("parse HTTPS proxy before subcommand");
    assert_eq!(before.https_proxy.as_deref(), Some("http://proxy.example:8443"));

    let after = CliArgs::try_parse_from([
        "pacquet",
        "install",
        "--http-proxy=http://proxy.example:8080",
        "--no-proxy=localhost,127.0.0.1",
    ])
    .expect("parse proxy settings after subcommand");
    assert_eq!(after.http_proxy.as_deref(), Some("http://proxy.example:8080"));
    assert_eq!(after.no_proxy.as_deref(), Some("localhost,127.0.0.1"));
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
fn loglevel_is_global_and_parses_on_either_side_of_the_subcommand() {
    for argv in [
        ["pacquet", "--loglevel", "error", "install"].as_slice(),
        ["pacquet", "install", "--loglevel=error"].as_slice(),
    ] {
        let parsed = CliArgs::try_parse_from(argv).expect("parses global --loglevel");
        assert_eq!(parsed.loglevel, Some(LogLevelSetting::Error));
    }
}

/// The exact invocation electron-builder's node-module collector runs;
/// rejecting it breaks Electron packaging
/// ([pnpm/pnpm#14024](https://github.com/pnpm/pnpm/issues/14024)).
#[test]
fn list_accepts_the_electron_builder_collector_invocation() {
    let parsed = CliArgs::try_parse_from([
        "pacquet",
        "list",
        "--prod",
        "--json",
        "--depth",
        "Infinity",
        "--loglevel",
        "error",
    ])
    .expect("parses the electron-builder `pnpm list` invocation");
    assert!(matches!(parsed.command, CliCommand::List(_)));
    assert_eq!(parsed.loglevel, Some(LogLevelSetting::Error));
}

#[test]
fn loglevel_silent_forces_the_silent_reporter_over_the_reporter_flag() {
    let parsed = CliArgs::try_parse_from([
        "pacquet",
        "--reporter",
        "append-only",
        "--loglevel",
        "silent",
        "install",
    ])
    .expect("parses --reporter with --loglevel silent");
    assert!(matches!(parsed.effective_reporter(), ReporterType::Silent));
}

#[test]
fn non_silent_loglevels_keep_the_selected_reporter() {
    let parsed =
        CliArgs::try_parse_from(["pacquet", "--loglevel", "warn", "install"]).expect("parses");
    assert!(matches!(parsed.effective_reporter(), ReporterType::Default));
}

#[test]
fn loglevel_rejects_unknown_values() {
    CliArgs::try_parse_from(["pacquet", "install", "--loglevel", "verbose"])
        .expect_err("unknown loglevel value must be rejected");
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
fn parallel_before_run_is_a_recursive_unsorted_run_option() {
    let mut parsed = CliArgs::try_parse_from(["pacquet", "--parallel", "run", "build"])
        .expect("parses --parallel before run");
    assert!(parsed.parallel);
    assert!(!parsed.recursive);
    parsed.validate_command_scoped_global_options().expect("run accepts --parallel");
    parsed.apply_parallel_run_options();
    assert!(parsed.recursive);
    assert!(parsed.no_sort);
    assert!(
        matches!(&parsed.command, CliCommand::Run(args) if args.script.as_slice() == ["build"]),
    );
}

#[test]
fn parallel_before_exec_is_a_recursive_unsorted_exec_option() {
    let mut parsed = CliArgs::try_parse_from(["pacquet", "--parallel", "exec", "echo"])
        .expect("parses --parallel before exec");
    parsed.validate_command_scoped_global_options().expect("exec accepts --parallel");
    parsed.apply_parallel_run_options();
    assert!(parsed.recursive);
    assert!(parsed.no_sort);
}

#[test]
fn parallel_after_run_script_is_forwarded_to_the_script() {
    let parsed = CliArgs::try_parse_from(["pacquet", "run", "build", "--parallel"])
        .expect("parses --parallel as a script argument");
    assert!(!parsed.parallel);
    assert!(
        matches!(&parsed.command, CliCommand::Run(args) if args.script.as_slice() == ["build", "--parallel"]),
    );
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
        ["pacquet", "-r", "--no-bail", "rebuild"].as_slice(),
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
        ["pacquet", "rebuild", "--resume-from", "pkg"].as_slice(),
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
fn recursive_by_default_command_is_promoted_inside_workspace() {
    let workspace = tempfile::tempdir().expect("creates workspace");
    std::fs::write(workspace.path().join("pnpm-workspace.yaml"), "packages: []\n")
        .expect("writes workspace manifest");
    for command in ["install", "import", "list", "why", "peers"] {
        let mut parsed = CliArgs::try_parse_from([
            "pacquet",
            "--dir",
            workspace.path().to_str().expect("UTF-8 path"),
            command,
        ])
        .expect("parses");

        parsed.promote_recursive_by_default();

        assert!(parsed.recursive, "{command} should be recursive inside a workspace");
    }
}

#[test]
fn color_accepts_modes_and_boolean_spellings() {
    for (value, expected) in [
        ("always", ColorMode::Always),
        ("auto", ColorMode::Auto),
        ("never", ColorMode::Never),
        ("true", ColorMode::Always),
        ("false", ColorMode::Never),
    ] {
        let color = format!("--color={value}");
        let parsed = CliArgs::try_parse_from(["pacquet", color.as_str(), "install"])
            .expect("color mode parses");
        assert_eq!(parsed.color, Some(expected));
    }
    let parsed =
        CliArgs::try_parse_from(["pacquet", "--color", "install"]).expect("bare color parses");
    assert_eq!(parsed.color, Some(ColorMode::Always));
}

#[test]
fn recursive_by_default_command_stays_non_recursive_outside_workspace() {
    let project = tempfile::tempdir().expect("creates project");
    for command in ["list", "why", "peers"] {
        let mut parsed = CliArgs::try_parse_from([
            "pacquet",
            "--dir",
            project.path().to_str().expect("UTF-8 path"),
            command,
        ])
        .expect("parses");

        parsed.promote_recursive_by_default();

        assert!(!parsed.recursive, "{command} should stay non-recursive outside a workspace");
    }
}

#[test]
fn commands_without_recursive_by_default_stay_non_recursive_in_workspace() {
    let workspace = tempfile::tempdir().expect("creates workspace");
    std::fs::write(workspace.path().join("pnpm-workspace.yaml"), "packages: []\n")
        .expect("writes workspace manifest");
    let mut parsed = CliArgs::try_parse_from([
        "pacquet",
        "--dir",
        workspace.path().to_str().expect("UTF-8 path"),
        "outdated",
    ])
    .expect("parses");

    parsed.promote_recursive_by_default();

    assert!(!parsed.recursive);
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

    let manifest = read_manifest_json(&manifest_path).expect("read manifest").expect("manifest");
    let package_manager =
        package_manager_to_sync(&manifest, root.path(), None).expect("sync package manager");

    assert_eq!(package_manager.specifier, ">=0.0.0");
    assert_eq!(
        package_manager.version,
        current_source_pnpm_version().expect("source pnpm version"),
    );
}

/// The range is built from `PNPM_VERSION` so the source checkout's version
/// (a different major) can never satisfy it and answer first.
#[test]
fn package_manager_to_sync_records_the_running_version_for_a_satisfied_range_pin() {
    let root = TempDir::new().expect("tmp dir");
    let manifest_path = root.path().join("package.json");
    let range = format!("^{}", pnpm_config::PNPM_VERSION);
    std::fs::write(
        &manifest_path,
        format!(
            r#"{{"devEngines":{{"packageManager":{{"name":"pnpm","version":"{range}","onFail":"download"}}}}}}"#,
        ),
    )
    .expect("write manifest");

    let manifest = read_manifest_json(&manifest_path).expect("read manifest").expect("manifest");
    let package_manager =
        package_manager_to_sync(&manifest, root.path(), None).expect("sync package manager");

    assert_eq!(package_manager.specifier, range);
    assert_eq!(package_manager.version, pnpm_config::PNPM_VERSION);
}

#[test]
fn package_manager_to_sync_records_nothing_for_a_pin_nothing_satisfies() {
    let root = TempDir::new().expect("tmp dir");
    let manifest_path = root.path().join("package.json");
    std::fs::write(
        &manifest_path,
        r#"{"devEngines":{"packageManager":{"name":"pnpm","version":"^999.0.0","onFail":"download"}}}"#,
    )
    .expect("write manifest");

    let manifest = read_manifest_json(&manifest_path).expect("read manifest").expect("manifest");
    assert_eq!(package_manager_to_sync(&manifest, root.path(), None), None);
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

/// Returns the canonicalized root too: a temp dir is a symlink on some
/// platforms, so a `--dir` redirect would not compare equal otherwise.
fn workspace_fixture() -> (TempDir, std::path::PathBuf) {
    let root = TempDir::new().expect("tmp dir");
    std::fs::write(root.path().join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    std::fs::create_dir_all(root.path().join("packages/a")).expect("create project dir");
    let canonical = dunce::canonicalize(root.path()).expect("canonicalize root");
    (root, canonical)
}

#[test]
fn workspace_root_is_global_and_parses_on_either_side_of_the_subcommand() {
    for argv in [
        ["pacquet", "--workspace-root", "add", "foo"].as_slice(),
        ["pacquet", "add", "foo", "--workspace-root"].as_slice(),
        ["pacquet", "-w", "add", "foo"].as_slice(),
        ["pacquet", "add", "foo", "-w"].as_slice(),
    ] {
        let parsed = CliArgs::try_parse_from(argv).expect("parses global --workspace-root");
        assert!(parsed.workspace_root, "{argv:?}");
        assert!(matches!(parsed.command, CliCommand::Add(_)));
    }
}

#[test]
fn workspace_root_points_dir_at_the_workspace_root() {
    let (root, canonical) = workspace_fixture();
    let subdir = root.path().join("packages/a");

    let mut args =
        CliArgs::try_parse_from(["pacquet", "add", "foo", "-w", "-C", &subdir.to_string_lossy()])
            .expect("parses");
    args.apply_workspace_root().expect("redirects to the workspace root");

    assert_eq!(args.dir, canonical);
}

#[test]
fn workspace_root_leaves_dir_alone_when_not_requested() {
    let (root, _canonical) = workspace_fixture();
    let subdir = root.path().join("packages/a");

    let mut args =
        CliArgs::try_parse_from(["pacquet", "add", "foo", "-C", &subdir.to_string_lossy()])
            .expect("parses");
    args.apply_workspace_root().expect("no-op without --workspace-root");

    assert_eq!(args.dir, subdir);
}

/// Every subcommand declaring `--global`. A new one added without wiring
/// it into [`CliCommand::is_global`] slips past the conflict check.
const GLOBAL_SUBCOMMAND_ARGV: [&[&str]; 12] = [
    &["add", "foo"],
    &["approve-builds"],
    &["bin"],
    &["config", "get", "store-dir"],
    &["list"],
    &["ll"],
    &["outdated"],
    &["prefix"],
    &["remove", "foo"],
    &["root"],
    &["runtime", "use", "node@20"],
    &["update"],
];

#[test]
fn workspace_root_conflicts_with_global_for_every_subcommand() {
    let (root, _canonical) = workspace_fixture();

    // Both spellings: pnpm accepts `-g` wherever it accepts `--global`, so a
    // subcommand declaring only the long form fails in the parser instead of
    // reaching the conflict check (pnpm/pnpm#13310).
    for global in ["--global", "-g"] {
        for subcommand in GLOBAL_SUBCOMMAND_ARGV {
            let argv = std::iter::once("pacquet")
                .chain(subcommand.iter().copied())
                .chain(["-w", global, "-C"])
                .chain([root.path().to_str().expect("utf-8 tmp dir")]);
            let mut args = CliArgs::try_parse_from(argv).unwrap_or_else(|error| {
                panic!("{subcommand:?} should parse with -w {global}: {error}");
            });
            let error = args
                .apply_workspace_root()
                .expect_err(&format!("{subcommand:?} must reject -w with {global}"));

            // The message carries what `dbg!` would, without 24 lines of it
            // on the way past.
            assert!(
                matches!(error, WorkspaceRootError::GlobalConflict),
                "{subcommand:?} {global}: {error:?}",
            );
        }
    }
}

#[test]
fn workspace_root_is_allowed_for_subcommands_without_global() {
    let (root, canonical) = workspace_fixture();

    for subcommand in [["install"].as_slice(), ["run", "build"].as_slice(), ["pack"].as_slice()] {
        // Ahead of the subcommand: `run` forwards everything after the
        // script name to the script, so a trailing `-w` would be the
        // script's argument rather than pnpm's.
        let argv = std::iter::once("pacquet")
            .chain(["-w", "-C"])
            .chain([root.path().to_str().expect("utf-8 tmp dir")])
            .chain(subcommand.iter().copied());
        let mut args = CliArgs::try_parse_from(argv).expect("parses");
        args.apply_workspace_root().unwrap_or_else(|error| {
            panic!("{subcommand:?} should accept -w: {error}");
        });

        assert_eq!(args.dir, canonical, "{subcommand:?}");
    }
}

#[test]
fn workspace_root_requires_a_workspace() {
    let outside = TempDir::new().expect("tmp dir");

    let mut args = CliArgs::try_parse_from([
        "pacquet",
        "add",
        "foo",
        "-w",
        "-C",
        &outside.path().to_string_lossy(),
    ])
    .expect("parses");
    let error = args.apply_workspace_root().expect_err("no workspace to redirect to");

    dbg!(&error);
    assert!(matches!(error, WorkspaceRootError::NotInWorkspace));
}

/// pnpm's `findWorkspaceDir` falls back to a lexical walk when
/// `fs.realpath` fails, so erroring here would diverge.
#[test]
fn workspace_root_tolerates_a_dir_that_does_not_exist() {
    let (root, canonical) = workspace_fixture();
    let missing = canonical.join("packages/does-not-exist");

    let mut args =
        CliArgs::try_parse_from(["pacquet", "add", "foo", "-w", "-C", &missing.to_string_lossy()])
            .expect("parses");
    args.apply_workspace_root().expect("redirects to the workspace root anyway");

    assert_eq!(args.dir, canonical);
    drop(root); // cleanup
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

#[test]
fn add_ignore_pnpmfile_flag_applies_to_config() {
    let mut config = pnpm_config::Config::default();
    add_args(&["pacquet", "add", "foo"]).apply_cli_config(&mut config);
    assert!(!config.ignore_pnpmfile, "flag absent → config unchanged");

    add_args(&["pacquet", "add", "foo", "--ignore-pnpmfile"]).apply_cli_config(&mut config);
    assert!(config.ignore_pnpmfile, "flag present → config set");
}

/// The install-family commands pnpm accepts `--ignore-pnpmfile` on.
/// `remove`, `prune`, `import`, `rebuild`, and `link` reject it there,
/// so they must reject it here too.
#[test]
fn every_command_pnpm_takes_ignore_pnpmfile_on_takes_it() {
    for argv in [
        ["pacquet", "install", "--ignore-pnpmfile"].as_slice(),
        ["pacquet", "add", "foo", "--ignore-pnpmfile"].as_slice(),
        ["pacquet", "update", "--ignore-pnpmfile"].as_slice(),
        ["pacquet", "dedupe", "--ignore-pnpmfile"].as_slice(),
        ["pacquet", "fetch", "--ignore-pnpmfile"].as_slice(),
        ["pacquet", "unlink", "--ignore-pnpmfile"].as_slice(),
        ["pacquet", "deploy", "--ignore-pnpmfile", "out"].as_slice(),
        ["pacquet", "ci", "--ignore-pnpmfile"].as_slice(),
        ["pacquet", "install-test", "--ignore-pnpmfile"].as_slice(),
    ] {
        CliArgs::try_parse_from(argv).unwrap_or_else(|err| panic!("{argv:?} parses: {err}"));
    }

    for argv in [
        ["pacquet", "remove", "foo", "--ignore-pnpmfile"].as_slice(),
        ["pacquet", "prune", "--ignore-pnpmfile"].as_slice(),
        ["pacquet", "import", "--ignore-pnpmfile"].as_slice(),
        ["pacquet", "rebuild", "--ignore-pnpmfile"].as_slice(),
        ["pacquet", "link", "--ignore-pnpmfile"].as_slice(),
    ] {
        CliArgs::try_parse_from(argv)
            .err()
            .unwrap_or_else(|| panic!("{argv:?} is rejected, as pnpm rejects it"));
    }
}

/// <https://github.com/pnpm/pnpm/issues/14107>
#[test]
fn dedupe_takes_the_install_options_pnpm_documents_for_it() {
    let args = dedupe_args(&[
        "pacquet",
        "dedupe",
        "--lockfile-only",
        "--ignore-scripts",
        "--offline",
        "--prefer-offline",
    ]);
    assert!(args.lockfile_only);
    assert!(args.ignore_scripts);
    assert!(args.offline);
    assert!(args.prefer_offline);

    let mut config = pnpm_config::Config::default();
    args.apply_cli_config(&mut config);
    assert!(config.ignore_scripts);
    assert!(config.offline);
    assert!(config.prefer_offline);

    let negated = dedupe_args(&[
        "pacquet",
        "dedupe",
        "--no-ignore-scripts",
        "--no-offline",
        "--no-prefer-offline",
    ]);
    let mut config = pnpm_config::Config {
        ignore_scripts: true,
        offline: true,
        prefer_offline: true,
        ..pnpm_config::Config::default()
    };
    negated.apply_cli_config(&mut config);
    assert!(!config.ignore_scripts, "the CLI negation turns a yaml `true` back off");
    assert!(!config.offline);
    assert!(!config.prefer_offline);
}

#[test]
fn dedupe_ignore_pnpmfile_flag_applies_to_config() {
    let mut config = pnpm_config::Config::default();
    dedupe_args(&["pacquet", "dedupe"]).apply_cli_config(&mut config);
    assert!(!config.ignore_pnpmfile, "flag absent → config unchanged");

    dedupe_args(&["pacquet", "dedupe", "--ignore-pnpmfile"]).apply_cli_config(&mut config);
    assert!(config.ignore_pnpmfile, "flag present → config set");
}

#[test]
fn unlink_ignore_pnpmfile_flag_applies_to_config() {
    let mut config = pnpm_config::Config::default();
    unlink_args(&["pacquet", "unlink"]).apply_cli_config(&mut config);
    assert!(!config.ignore_pnpmfile, "flag absent → config unchanged");

    unlink_args(&["pacquet", "unlink", "--ignore-pnpmfile"]).apply_cli_config(&mut config);
    assert!(config.ignore_pnpmfile, "flag present → config set");
}

fn dedupe_args(argv: &[&str]) -> DedupeArgs {
    match CliArgs::try_parse_from(argv).expect("parses").command {
        CliCommand::Dedupe(dedupe) => dedupe,
        other => panic!("expected dedupe, got {other:?}"),
    }
}

fn unlink_args(argv: &[&str]) -> UnlinkArgs {
    match CliArgs::try_parse_from(argv).expect("parses").command {
        CliCommand::Unlink(unlink) => unlink,
        other => panic!("expected unlink, got {other:?}"),
    }
}

#[test]
fn get_and_set_are_top_level_spellings_of_the_config_subcommands() {
    for (alias, params) in
        [("get", ["store-dir"].as_slice()), ("set", ["store-dir", "/tmp/store"].as_slice())]
    {
        for (flag, expected_global, expected_location, expected_json) in config_flag_cases() {
            for flag_before_params in [true, false] {
                let mut argv = vec!["pacquet", alias];
                if flag_before_params {
                    argv.extend(flag);
                }
                argv.extend(params);
                if !flag_before_params {
                    argv.extend(flag);
                }

                match (alias, command(&argv)) {
                    ("get", CliCommand::Get(get)) => {
                        assert_eq!(get.args.key.as_deref(), Some("store-dir"), "{argv:?}");
                        assert_eq!(get.flags.global, expected_global, "{argv:?}");
                        assert_eq!(get.flags.location, expected_location, "{argv:?}");
                        assert_eq!(get.flags.json, expected_json, "{argv:?}");
                    }
                    ("set", CliCommand::Set(set)) => {
                        assert_eq!(set.args.key.as_deref(), Some("store-dir"), "{argv:?}");
                        assert_eq!(set.args.value.as_deref(), Some("/tmp/store"), "{argv:?}");
                        assert_eq!(set.flags.global, expected_global, "{argv:?}");
                        assert_eq!(set.flags.location, expected_location, "{argv:?}");
                        assert_eq!(set.flags.json, expected_json, "{argv:?}");
                    }
                    (_, command) => panic!("expected {alias}, got {command:?}"),
                }
            }
        }
    }
}

#[test]
fn config_flags_parse_on_either_side_of_the_subcommand() {
    for subcommand in [
        ["set", "registry", "https://registry.test"].as_slice(),
        ["get", "registry"].as_slice(),
        ["delete", "registry"].as_slice(),
        ["list"].as_slice(),
    ] {
        for (flag, expected_global, expected_location, expected_json) in config_flag_cases() {
            for flag_before_subcommand in [true, false] {
                let mut argv = vec!["pacquet", "config"];
                if flag_before_subcommand {
                    argv.extend(flag);
                }
                argv.extend(subcommand);
                if !flag_before_subcommand {
                    argv.extend(flag);
                }

                let CliCommand::Config(args) = command(&argv) else {
                    panic!("expected config");
                };
                assert_eq!(args.flags.global, expected_global, "{argv:?}");
                assert_eq!(args.flags.location, expected_location, "{argv:?}");
                assert_eq!(args.flags.json, expected_json, "{argv:?}");
                match (subcommand[0], args.command) {
                    ("set", ConfigSubcommand::Set(set)) => {
                        assert_eq!(set.key.as_deref(), Some("registry"), "{argv:?}");
                        assert_eq!(set.value.as_deref(), Some("https://registry.test"), "{argv:?}");
                    }
                    ("get", ConfigSubcommand::Get(get)) => {
                        assert_eq!(get.key.as_deref(), Some("registry"), "{argv:?}");
                    }
                    ("delete", ConfigSubcommand::Delete(delete)) => {
                        assert_eq!(delete.key.as_deref(), Some("registry"), "{argv:?}");
                    }
                    ("list", ConfigSubcommand::List(_)) => {}
                    (subcommand, command) => {
                        panic!("expected config {subcommand}, got {command:?}")
                    }
                }
            }
        }
    }
}

type ConfigFlagCase = (&'static [&'static str], bool, Option<ConfigLocation>, bool);

fn config_flag_cases() -> impl Iterator<Item = ConfigFlagCase> {
    [
        (["--global"].as_slice(), true, None, false),
        (["-g"].as_slice(), true, None, false),
        (["--location", "project"].as_slice(), false, Some(ConfigLocation::Project), false),
        (["--location", "global"].as_slice(), false, Some(ConfigLocation::Global), false),
        (["--json"].as_slice(), false, None, true),
    ]
    .into_iter()
}

#[test]
fn get_and_set_report_through_stderr_like_config_does() {
    for argv in [
        ["pacquet", "get", "store-dir"].as_slice(),
        ["pacquet", "set", "store-dir", "/tmp/store"].as_slice(),
        ["pacquet", "config", "get", "store-dir"].as_slice(),
    ] {
        assert!(command(argv).uses_stderr_reporter(), "{argv:?}");
    }
}

#[test]
fn env_collects_its_subcommand_and_arguments() {
    let CliCommand::Env(env) = command(&["pacquet", "env", "use", "24", "--global"]) else {
        panic!("expected env");
    };
    assert!(env.global);
    assert_eq!(env.params, ["use", "24"]);
}

#[test]
fn the_unimplemented_npm_commands_parse_instead_of_falling_through_to_a_script() {
    assert!(matches!(command(&["pacquet", "edit", "foo"]), CliCommand::Edit(_)));
    assert!(matches!(command(&["pacquet", "profile", "get"]), CliCommand::Profile(_)));
    assert!(matches!(
        command(&["pacquet", "token", "create", "--read-only"]),
        CliCommand::Token(_),
    ));
    assert!(matches!(command(&["pacquet", "xmas"]), CliCommand::Xmas(_)));
}

#[test]
fn store_status_and_add_are_subcommands_of_store() {
    let CliCommand::Store(StoreCommand::Status) = command(&["pacquet", "store", "status"]) else {
        panic!("expected store status");
    };

    let CliCommand::Store(StoreCommand::Add(add)) =
        command(&["pacquet", "store", "add", "express@4", "typescript@2.1.0"])
    else {
        panic!("expected store add");
    };
    assert_eq!(add.packages, ["express@4", "typescript@2.1.0"]);
}

/// `--production` is the setting name behind `--prod`, and pnpm accepts
/// it wherever `--prod` selects dependency groups — in a command line
/// typed by hand as much as in the install the verify-deps-before-run
/// gate reproduces
/// ([pnpm/pnpm#14147](https://github.com/pnpm/pnpm/issues/14147)).
#[test]
fn production_is_an_alias_of_prod() {
    for argv in [
        ["pacquet", "install", "--production"].as_slice(),
        ["pacquet", "fetch", "--production"].as_slice(),
        ["pacquet", "prune", "--production"].as_slice(),
        ["pacquet", "update", "--production"].as_slice(),
        ["pacquet", "sbom", "--sbom-format", "spdx", "--production"].as_slice(),
        ["pacquet", "list", "--production"].as_slice(),
        ["pacquet", "why", "--production", "foo"].as_slice(),
        ["pacquet", "audit", "--production"].as_slice(),
        ["pacquet", "licenses", "list", "--production"].as_slice(),
        ["pacquet", "outdated", "--production"].as_slice(),
    ] {
        CliArgs::try_parse_from(argv)
            .unwrap_or_else(|error| panic!("`{}` must parse: {error}", argv.join(" ")));
    }

    let groups = |argv: &[&str]| {
        install_args(argv).dependency_options.dependency_groups(true).collect::<Vec<_>>()
    };
    assert_eq!(
        groups(&["pacquet", "install", "--production"]),
        groups(&["pacquet", "install", "--prod"]),
    );
}

fn command(argv: &[&str]) -> CliCommand {
    CliArgs::try_parse_from(argv).expect("parses").command
}
