use super::{UpdateArgs, UpdateDependencyOptions};
use clap::Parser;
use pacquet_config::Config;
use pacquet_package_manifest::DependencyGroup;

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

fn options(prod: bool, dev: bool, no_optional: bool) -> UpdateDependencyOptions {
    UpdateDependencyOptions { prod, dev, no_optional }
}

#[test]
fn no_flags_includes_all_groups() {
    let groups = options(false, false, false).include_direct();
    assert_eq!(
        groups,
        vec![DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
    );
}

#[test]
fn prod_includes_only_dependencies() {
    let groups = options(true, false, false).include_direct();
    assert_eq!(groups, vec![DependencyGroup::Prod]);
}

#[test]
fn dev_includes_only_dev_dependencies() {
    let groups = options(false, true, false).include_direct();
    assert_eq!(groups, vec![DependencyGroup::Dev]);
}

#[test]
fn no_optional_alone_does_not_drop_optional() {
    let groups = options(false, false, true).include_direct();
    assert_eq!(
        groups,
        vec![DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional],
    );
}

#[test]
fn prod_with_no_optional_drops_optional() {
    let groups = options(true, false, true).include_direct();
    assert_eq!(groups, vec![DependencyGroup::Prod]);
}

#[test]
fn github_actions_are_opt_in_except_for_interactive_updates() {
    let include_direct = vec![DependencyGroup::Prod, DependencyGroup::Dev];
    let mut config = Config::new();

    assert!(!update_args(&[]).should_update_github_actions(&config, &include_direct));
    assert!(
        update_args(&["--include-github-actions"])
            .should_update_github_actions(&config, &include_direct),
    );
    assert!(update_args(&["--interactive"]).should_update_github_actions(&config, &include_direct));

    config.update_config.github_actions = Some(true);
    assert!(update_args(&[]).should_update_github_actions(&config, &include_direct));
    assert!(
        !update_args(&["--prod"]).should_update_github_actions(&config, &[DependencyGroup::Prod],),
    );

    // An explicit `false` opts interactive updates out of GitHub Actions,
    // but never overrides the explicit `--include-github-actions` flag.
    config.update_config.github_actions = Some(false);
    assert!(
        !update_args(&["--interactive"]).should_update_github_actions(&config, &include_direct),
    );
    assert!(
        update_args(&["--include-github-actions"])
            .should_update_github_actions(&config, &include_direct),
    );
}
