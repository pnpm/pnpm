use super::{
    CliArgs,
    add::AddArgs,
    cli_command::{CliCommand, options::WorkspaceRootError},
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
    version::VersionArgs,
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

fn version_args(argv: &[&str]) -> VersionArgs {
    match CliArgs::try_parse_from(argv).expect("parses").command {
        CliCommand::Version(version) => version,
        other => panic!("expected version, got {other:?}"),
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
fn add_allow_build_collects_repeated_values() {
    let args =
        add_args(&["pacquet", "add", "foo", "--allow-build=esbuild", "--allow-build", "sharp"]);
    assert_eq!(args.allow_build, ["esbuild", "sharp"]);
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
fn version_message_short_flag_is_an_alias_of_message() {
    let short = version_args(&["pacquet", "version", "patch", "-m", "release %s"]);
    let long = version_args(&["pacquet", "version", "patch", "--message", "release %s"]);
    assert_eq!(short.message.as_deref(), Some("release %s"));
    assert_eq!(short.message, long.message);
    assert_eq!(short.params, ["patch"]);
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
        for case in config_flag_cases() {
            for flag_first in [true, false] {
                let argv = argv_with_flag(&[alias], params, case.0, flag_first);
                assert_top_level_config_flags(alias, &argv, case);
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
        for case in config_flag_cases() {
            for flag_first in [true, false] {
                let argv = argv_with_flag(&["config"], subcommand, case.0, flag_first);
                assert_config_subcommand_flags(subcommand[0], &argv, case);
            }
        }
    }
}

/// `pacquet <leading…> <words…>` with `flag` on whichever side of the
/// subcommand `flag_first` asks for.
fn argv_with_flag<'arg>(
    leading: &[&'arg str],
    words: &[&'arg str],
    flag: &[&'arg str],
    flag_first: bool,
) -> Vec<&'arg str> {
    let mut argv = vec!["pacquet"];
    argv.extend(leading);
    if flag_first {
        argv.extend(flag);
    }
    argv.extend(words);
    if !flag_first {
        argv.extend(flag);
    }
    argv
}

fn assert_top_level_config_flags(alias: &str, argv: &[&str], case: ConfigFlagCase) {
    let (_, expected_global, expected_location, expected_json) = case;
    match (alias, command(argv)) {
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

fn assert_config_subcommand_flags(subcommand: &str, argv: &[&str], case: ConfigFlagCase) {
    let (_, expected_global, expected_location, expected_json) = case;
    let CliCommand::Config(args) = command(argv) else {
        panic!("expected config");
    };
    assert_eq!(args.flags.global, expected_global, "{argv:?}");
    assert_eq!(args.flags.location, expected_location, "{argv:?}");
    assert_eq!(args.flags.json, expected_json, "{argv:?}");
    match (subcommand, args.command) {
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

mod global_options;
