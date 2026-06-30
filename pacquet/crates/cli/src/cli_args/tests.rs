use super::{
    CliArgs,
    cli_command::CliCommand,
    package_manager::{
        current_source_pnpm_version, package_manager_to_sync, parse_package_manager,
    },
};
use clap::Parser;
use tempfile::TempDir;

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
