use super::{
    CliArgs, CliCommand, ColorMode, GLOBAL_SUBCOMMAND_ARGV, LogLevelSetting, Parser, Path,
    RecursionLimit, ReporterType, SummaryScope, TempDir, WorkspaceRootError,
    default_reporter_summary_scope, workspace_fixture,
};

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
    for command in ["install", "dedupe", "import", "list", "why", "peers"] {
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
    for command in ["dedupe", "list", "why", "peers"] {
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
