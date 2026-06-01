use super::UpdateDependencyOptions;
use pacquet_package_manifest::DependencyGroup;

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
    // Mirrors pnpm's `makeIncludeDependenciesFromCLI` with
    // `production === true`: dependencies only.
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
    // Faithful to pnpm's quirk: `--no-optional` only excludes
    // optionalDependencies when combined with `--prod` or `--dev`,
    // because the inclusion formula reads the raw CLI flags.
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
