use super::{CliArgs, CliCommand, package_manager_to_sync};
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
        super::current_source_pnpm_version().expect("source pnpm version"),
    );
}
