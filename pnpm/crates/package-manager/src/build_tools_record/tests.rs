use super::{PriorRecords, apply_records, git_hosted_metadata_keys};
use pnpm_lockfile::{Lockfile, PackageKey};
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use text_block_macros::text_block;

const RECORDED: &str = text_block! {
    "lockfileVersion: '9.0'"
    "settings:"
    "  autoInstallPeers: true"
    "  excludeLinksFromLockfile: false"
    "  lockBuildTools: true"
    "importers:"
    "  .:"
    "    dependencies:"
    "      built:"
    "        specifier: git+file:///repos/built#1111111111111111111111111111111111111111"
    "        version: git+file:///repos/built#1111111111111111111111111111111111111111"
    "      plain:"
    "        specifier: git+file:///repos/plain#2222222222222222222222222222222222222222"
    "        version: git+file:///repos/plain#2222222222222222222222222222222222222222"
    "      is-positive:"
    "        specifier: 1.0.0"
    "        version: 1.0.0"
    "packages:"
    "  built@git+file:///repos/built#1111111111111111111111111111111111111111:"
    "    resolution: {commit: '1111111111111111111111111111111111111111', repo: file:///repos/built, type: git}"
    "    version: 1.0.0"
    "  plain@git+file:///repos/plain#2222222222222222222222222222222222222222:"
    "    resolution: {commit: '2222222222222222222222222222222222222222', repo: file:///repos/plain, type: git}"
    "    version: 1.0.0"
    "  is-positive@1.0.0:"
    "    resolution: {integrity: sha512-mQ==}"
    "  yarn@tool:1.22.22:"
    "    resolution: {integrity: sha512-yQ==, tarball: https://registry.npmjs.org/yarn/-/yarn-1.22.22.tgz}"
    "snapshots:"
    "  built@git+file:///repos/built#1111111111111111111111111111111111111111:"
    "    buildDependencies:"
    "      yarn: tool:1.22.22"
    "  plain@git+file:///repos/plain#2222222222222222222222222222222222222222: {}"
    "  is-positive@1.0.0: {}"
};

fn parse(text: &str) -> Lockfile {
    serde_saphyr::from_str(text).expect("parse lockfile")
}

fn key(text: &str) -> PackageKey {
    text.parse().expect("parse key")
}

const BUILT: &str = "built@git+file:///repos/built#1111111111111111111111111111111111111111";
const PLAIN: &str = "plain@git+file:///repos/plain#2222222222222222222222222222222222222222";

/// A lockfile as the resolver writes it: the records dropped.
fn unrecorded() -> Lockfile {
    let mut lockfile = parse(RECORDED);
    for snapshot in lockfile.snapshots
        .iter_mut()
        .flatten()
        .map(|(_, snapshot)| snapshot)
    {
        snapshot.build_dependencies = None;
    }
    let tool = key("yarn@tool:1.22.22");
    lockfile.packages
        .as_mut()
        .unwrap()
        .remove(&tool);
    lockfile.snapshots
        .as_mut()
        .unwrap()
        .remove(&tool);
    lockfile
}

#[test]
fn only_git_hosted_packages_are_examined() {
    let keys: Vec<String> = git_hosted_metadata_keys(&unrecorded())
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(keys, [BUILT, PLAIN]);
}

/// A package the previous lockfile already examined keeps what it found,
/// a record or the absence of one.
#[test]
fn records_carry_over_for_an_unchanged_package() {
    let prior = parse(RECORDED);
    let lockfile = unrecorded();

    let built = PriorRecords::of(&prior)
        .carried(&lockfile, &key(BUILT))
        .expect("carried");
    let built = built.expect("a record");
    let tools: Vec<String> = built
        .values()
        .map(|(tool_key, _)| tool_key.to_string())
        .collect();
    assert_eq!(tools, ["yarn@tool:1.22.22"]);
    assert_eq!(
        PriorRecords::of(&prior)
            .carried(&lockfile, &key(PLAIN))
            .map(|record| record.is_none()),
        Some(true),
    );
}

#[test]
fn a_package_the_previous_lockfile_does_not_hold_is_examined_again() {
    let prior = parse(RECORDED);
    let mut lockfile = unrecorded();
    let packages = lockfile.packages.as_mut().unwrap();
    let mut metadata = packages.remove(&key(BUILT)).unwrap();
    let moved = "built@git+file:///repos/built#3333333333333333333333333333333333333333";
    if let pnpm_lockfile::LockfileResolution::Git(git) = &mut metadata.resolution {
        git.commit = "3333333333333333333333333333333333333333".to_string();
    }
    packages.insert(key(moved), metadata);

    let carried = PriorRecords::of(&prior).carried(&lockfile, &key(moved));
    assert!(carried.is_none());
}

#[test]
fn applied_records_round_trip_to_the_recorded_lockfile() {
    let prior = parse(RECORDED);
    let mut lockfile = unrecorded();
    let records: HashMap<_, _> = [BUILT, PLAIN]
        .into_iter()
        .map(|metadata_key| {
            let record = PriorRecords::of(&prior)
                .carried(&lockfile, &key(metadata_key))
                .expect("carried");
            (key(metadata_key), record)
        })
        .collect();

    apply_records(&mut lockfile, &records);

    assert_eq!(lockfile, prior);
}

/// A package that needs a tool the previous lockfile records reuses that
/// version rather than resolving the tool again.
#[test]
fn recorded_tools_are_offered_for_reuse() {
    let prior = parse(RECORDED);
    let tools: Vec<String> = PriorRecords::of(&prior)
        .tools()
        .keys()
        .map(ToString::to_string)
        .collect();
    assert_eq!(tools, ["yarn@tool:1.22.22"]);
}

/// A tool entry no snapshot names is never handed to a new package.
#[test]
fn unreferenced_tool_entries_are_not_offered() {
    let mut prior = parse(RECORDED);
    for (_, snapshot) in prior.snapshots.iter_mut().flatten() {
        snapshot.build_dependencies = None;
    }
    assert!(PriorRecords::of(&prior).tools().is_empty());
}

/// A carried record whose tool is not that tool's own artifact sends the
/// package back to be examined.
#[test]
fn an_invalid_carried_tool_is_examined_again() {
    let prior = parse(&RECORDED.replace(
        "tarball: https://registry.npmjs.org/yarn/-/yarn-1.22.22.tgz",
        "tarball: 'file:./yarn.tgz'",
    ));
    let carried = PriorRecords::of(&prior).carried(&unrecorded(), &key(BUILT));
    assert!(carried.is_none());
}

/// Only a git-hosted package's record offers its tools: one an edited
/// lockfile attaches to a registry package is ignored.
#[test]
fn tools_a_registry_snapshot_names_are_not_offered() {
    let mut prior = parse(RECORDED);
    let snapshots = prior.snapshots.as_mut().unwrap();
    let recorded = snapshots[&key(BUILT)].build_dependencies.clone();
    snapshots
        .get_mut(&key(BUILT))
        .unwrap()
        .build_dependencies = None;
    snapshots
        .get_mut(&key("is-positive@1.0.0"))
        .unwrap()
        .build_dependencies = recorded;
    assert!(PriorRecords::of(&prior).tools().is_empty());
}
