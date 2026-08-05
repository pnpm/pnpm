use super::resolved_package_versions;
use pacquet_lockfile::Lockfile;
use std::path::Path;

/// A package resolved only from a non-semver source registers its name
/// with an empty version set, so the cleanup pass keeps its bare-name
/// `minimumReleaseAgeExclude` entry while still pruning versioned ones.
#[test]
fn registers_non_semver_packages_by_name_only() {
    let lockfile = Lockfile::parse(
        "lockfileVersion: '9.0'\n\
         snapshots:\n  \
         foo@https://codeload.github.com/owner/repo/tarball/deadbeef: {}\n  \
         bar@1.0.0: {}\n",
        Path::new("pnpm-lock.yaml"),
    )
    .expect("lockfile parses")
    .expect("lockfile is non-empty");

    let resolved = resolved_package_versions(&lockfile);

    assert_eq!(resolved.get("foo").map(std::collections::BTreeSet::len), Some(0));
    assert_eq!(resolved.get("bar").map(|versions| versions.contains("1.0.0")), Some(true));
}

/// A registry-qualified snapshot key (`<name>@<registryName>:<version>`)
/// registers the version after the prefix, so a versioned
/// `minimumReleaseAgeExclude` entry for it survives the cleanup pass.
#[test]
fn registers_the_version_of_a_registry_qualified_key() {
    let lockfile = Lockfile::parse(
        "lockfileVersion: '9.0'\nsnapshots:\n  foo@myregistry:1.0.0: {}\n",
        Path::new("pnpm-lock.yaml"),
    )
    .expect("lockfile parses")
    .expect("lockfile is non-empty");

    let resolved = resolved_package_versions(&lockfile);

    assert_eq!(resolved.get("foo").map(|versions| versions.contains("1.0.0")), Some(true));
}
