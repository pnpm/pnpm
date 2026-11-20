use crate::Lockfile;
use pnpm_package_manifest::DependencyGroup;
use pretty_assertions::assert_eq;

const ALL_GROUPS: [DependencyGroup; 3] =
    [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];

fn injected(lockfile: &str, importer_id: &str) -> Vec<String> {
    injected_in_groups(lockfile, importer_id, ALL_GROUPS)
}

fn injected_in_groups(
    lockfile: &str,
    importer_id: &str,
    groups: impl IntoIterator<Item = DependencyGroup> + 'static,
) -> Vec<String> {
    let lockfile: Lockfile = serde_saphyr::from_str(lockfile).unwrap();
    let mut aliases = lockfile
        .injected_dependencies(importer_id, groups)
        .map(|dependency| dependency.alias.to_string())
        .collect::<Vec<_>>();
    aliases.sort();
    aliases
}

const LOCKFILE: &str = "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      injected-prod:
        specifier: workspace:*
        version: file:injected-prod
      linked:
        specifier: workspace:*
        version: link:linked
      local-tarball:
        specifier: file:local.tgz
        version: file:local.tgz
      registry:
        specifier: 1.0.0
        version: 1.0.0
      renamed:
        specifier: workspace:injected-renamed@*
        version: injected-renamed@file:injected-renamed
    devDependencies:
      injected-dev:
        specifier: file:injected-dev
        version: file:injected-dev(peer@1.0.0)

  other: {}

packages:

  injected-dev@file:injected-dev:
    resolution: {directory: injected-dev, type: directory}

  injected-prod@file:injected-prod:
    resolution: {directory: injected-prod, type: directory}

  injected-renamed@file:injected-renamed:
    resolution: {directory: injected-renamed, type: directory}

  local-tarball@file:local.tgz:
    resolution: {integrity: sha512-AAAA, tarball: file:local.tgz}

  registry@1.0.0:
    resolution: {integrity: sha512-AAAA}

snapshots:

  injected-dev@file:injected-dev(peer@1.0.0): {}

  injected-prod@file:injected-prod: {}

  injected-renamed@file:injected-renamed: {}

  local-tarball@file:local.tgz: {}

  registry@1.0.0: {}
";

#[test]
fn directory_copies_are_injected() {
    assert_eq!(injected(LOCKFILE, "."), ["injected-dev", "injected-prod", "renamed"]);
}

#[test]
fn only_the_given_groups_are_read() {
    assert_eq!(injected_in_groups(LOCKFILE, ".", [DependencyGroup::Dev]), ["injected-dev"]);
}

#[test]
fn an_importer_without_dependencies_injects_nothing() {
    assert!(injected(LOCKFILE, "other").is_empty());
}

#[test]
fn a_missing_importer_injects_nothing() {
    assert!(injected(LOCKFILE, "missing").is_empty());
}
