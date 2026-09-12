use super::{UpdateArgs, UpdateDependencyOptions};
use clap::Parser;
use pnpm_config::Config;
use pnpm_package_manifest::DependencyGroup;

#[derive(Debug, Parser)]
struct UpdateArgsHarness {
    #[clap(flatten)]
    args: UpdateArgs,
}

fn update_args(args: &[&str]) -> UpdateArgs {
    UpdateArgsHarness::try_parse_from(std::iter::once("pacquet-test").chain(args.iter().copied()))
        .expect("parse update arguments")
        .args
}

fn options(prod: bool, dev: bool, optional: bool, no_optional: bool) -> UpdateDependencyOptions {
    UpdateDependencyOptions { prod, dev, optional, no_optional }
}

#[test]
fn no_flags_includes_all_groups() {
    let groups = options(false, false, false, false).include_direct();
    assert_eq!(
        groups,
        vec![DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
    );
}

#[test]
fn prod_includes_only_dependencies() {
    let groups = options(true, false, false, false).include_direct();
    assert_eq!(groups, vec![DependencyGroup::Prod]);
}

#[test]
fn dev_includes_only_dev_dependencies() {
    let groups = options(false, true, false, false).include_direct();
    assert_eq!(groups, vec![DependencyGroup::Dev]);
}

#[test]
fn no_optional_alone_does_not_drop_optional() {
    let groups = options(false, false, false, true).include_direct();
    assert_eq!(
        groups,
        vec![DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
    );
}

#[test]
fn prod_with_no_optional_drops_optional() {
    let groups = options(true, false, false, true).include_direct();
    assert_eq!(groups, vec![DependencyGroup::Prod]);
}

#[test]
fn optional_includes_only_optional_dependencies() {
    let groups = options(false, false, true, false).include_direct();
    assert_eq!(groups, vec![DependencyGroup::Optional]);
}

#[test]
fn github_actions_are_opt_in_for_every_update() {
    let include_direct = vec![DependencyGroup::Prod, DependencyGroup::Dev];
    let mut config = Config::new();

    assert!(!update_args(&[]).should_update_github_actions(&config, &include_direct));
    assert!(
        !update_args(&["--interactive"]).should_update_github_actions(&config, &include_direct),
    );
    assert!(
        update_args(&["--include-github-actions"])
            .should_update_github_actions(&config, &include_direct),
    );
    assert!(
        update_args(&["--interactive", "--include-github-actions"])
            .should_update_github_actions(&config, &include_direct),
    );

    config.update_config.github_actions = Some(true);
    assert!(update_args(&[]).should_update_github_actions(&config, &include_direct));
    assert!(update_args(&["--interactive"]).should_update_github_actions(&config, &include_direct));
    assert!(
        !update_args(&["--prod"]).should_update_github_actions(&config, &[DependencyGroup::Prod],),
    );

    config.update_config.github_actions = Some(false);
    assert!(
        !update_args(&["--interactive"]).should_update_github_actions(&config, &include_direct),
    );
    assert!(
        update_args(&["--include-github-actions"])
            .should_update_github_actions(&config, &include_direct),
    );
}

#[test]
fn workspace_option_is_checked_before_anything_is_read() {
    let workspace_root = std::path::Path::new("/workspace");

    assert_eq!(
        update_args(&[]).check_workspace_option(Some(workspace_root)).expect("no flag"),
        None,
    );
    assert_eq!(
        update_args(&["--workspace"]).check_workspace_option(Some(workspace_root)).expect("linked"),
        Some(workspace_root),
    );

    let outside = update_args(&["--workspace"])
        .check_workspace_option(None)
        .expect_err("--workspace outside a workspace");
    assert_eq!(outside.to_string(), "--workspace can only be used inside a workspace");

    let with_latest = update_args(&["--workspace", "--latest"])
        .check_workspace_option(Some(workspace_root))
        .expect_err("--workspace with --latest");
    assert_eq!(with_latest.to_string(), "Cannot use --latest with --workspace simultaneously");
}

#[test]
fn ignore_pnpmfile_flag_applies_to_config() {
    let mut config = Config::default();
    update_args(&[]).apply_cli_config(&mut config);
    assert!(!config.ignore_pnpmfile, "flag absent → config unchanged");

    update_args(&["--ignore-pnpmfile"]).apply_cli_config(&mut config);
    assert!(config.ignore_pnpmfile, "flag present → config set");
}

#[test]
fn ignore_scripts_flags_apply_to_config() {
    let mut config = Config::default();
    assert!(!config.ignore_scripts);

    config.ignore_scripts = true;
    update_args(&[]).apply_cli_config(&mut config);
    assert!(config.ignore_scripts, "flags absent -> config unchanged");

    config.ignore_scripts = false;
    update_args(&["--ignore-scripts"]).apply_cli_config(&mut config);
    assert!(config.ignore_scripts, "--ignore-scripts enables the setting");

    update_args(&["--no-ignore-scripts"]).apply_cli_config(&mut config);
    assert!(!config.ignore_scripts, "--no-ignore-scripts disables the setting");
}

#[test]
fn pnpr_server_flag_applies_to_config() {
    let mut config = Config::default();

    update_args(&["--pnpr-server", "https://pnpr.example.test/"]).apply_cli_config(&mut config);

    assert_eq!(config.pnpr_server.as_deref(), Some("https://pnpr.example.test/"));
}

#[test]
fn patches_is_a_selectorless_update_mode() {
    let patches = update_args(&["--patches"]);
    assert!(patches.patches);
    patches.check_patches_options().expect("standalone --patches");

    for args in [
        &["--patches", "foo"][..],
        &["--patches", "--latest"][..],
        &["--patches", "--interactive"][..],
        &["--patches", "--global"][..],
    ] {
        let error =
            update_args(args).check_patches_options().expect_err("--patches combination must fail");
        assert_eq!(
            error.to_string(),
            "--patches cannot be combined with package selectors, --latest, --interactive, or --global",
        );
    }
}

#[test]
fn constrained_patch_refresh_stays_on_the_client() {
    let all_groups = vec![DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];
    let prod_only = vec![DependencyGroup::Prod];

    assert!(update_args(&["--patches"]).can_delegate_patch_refresh(false, &all_groups));
    assert!(
        !update_args(&["--patches", "--depth", "0"]).can_delegate_patch_refresh(false, &all_groups),
    );
    assert!(!update_args(&["--patches"]).can_delegate_patch_refresh(true, &all_groups));
    assert!(!update_args(&["--patches"]).can_delegate_patch_refresh(false, &prod_only));
}
