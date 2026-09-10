use super::{DIGEST_A, DIGEST_B, depends_on, registry_metadata};
use pnpm_lockfile::{PackageKey, PackageMetadata, SnapshotEntry};
use pretty_assertions::{assert_eq, assert_ne};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

/// The cached suffix map decides where every package in the store
/// lives, so the key has to move whenever anything the suffixes are
/// derived from moves. It is taken from the dep graph rather than from
/// the lockfile file, so this walks the input classes that graph and
/// its hasher read.
#[test]
fn layout_cache_fingerprint_tracks_every_derivation_input() {
    let key: PackageKey = "foo@1.0.0".parse().expect("parse key");
    let dep: PackageKey = "bar@2.0.0".parse().expect("parse key");
    let snapshots =
        HashMap::from([(key.clone(), depends_on("bar")), (dep.clone(), SnapshotEntry::default())]);
    let packages = HashMap::from([
        (key.clone(), registry_metadata("a")),
        (dep.clone(), registry_metadata("b")),
    ]);
    let policy = crate::AllowBuildPolicy::default();
    let dir = PathBuf::from("/tmp/project");
    let fingerprint = |snapshots: &HashMap<PackageKey, SnapshotEntry>,
                       packages: &HashMap<PackageKey, PackageMetadata>,
                       engine: Option<&str>,
                       policy: Option<&crate::AllowBuildPolicy>,
                       dir: Option<&Path>| {
        super::super::GvsHasher::new(snapshots, Some(packages), engine, policy, dir)
            .fingerprint(snapshots)
    };
    let baseline =
        fingerprint(&snapshots, &packages, Some("linux;x64;22"), Some(&policy), Some(&dir));

    assert_eq!(
        baseline,
        fingerprint(&snapshots, &packages, Some("linux;x64;22"), Some(&policy), Some(&dir)),
        "the same inputs must fingerprint identically",
    );
    assert_ne!(
        baseline,
        fingerprint(&snapshots, &packages, Some("linux;x64;24"), Some(&policy), Some(&dir)),
        "a different node major must retire the entry",
    );
    assert_ne!(
        baseline,
        fingerprint(&snapshots, &packages, None, Some(&policy), Some(&dir)),
        "an absent engine and an empty one must not share an entry",
    );
    assert_ne!(
        baseline,
        fingerprint(&snapshots, &packages, Some(""), Some(&policy), Some(&dir)),
        "an empty engine string is its own input",
    );
    assert_ne!(
        baseline,
        fingerprint(&snapshots, &packages, Some("linux;x64;22"), Some(&policy), None),
        "the project scope enters every local directory snapshot's hash",
    );

    let mut rewired = snapshots.clone();
    rewired.insert(key.clone(), SnapshotEntry::default());
    assert_ne!(
        baseline,
        fingerprint(&rewired, &packages, Some("linux;x64;22"), Some(&policy), Some(&dir)),
        "dropping a dependency edge must retire the entry",
    );

    let mut republished = packages.clone();
    republished.insert(dep, registry_metadata("c"));
    assert_ne!(
        baseline,
        fingerprint(&snapshots, &republished, Some("linux;x64;22"), Some(&policy), Some(&dir)),
        "a changed integrity must retire the entry, even at the same version",
    );

    let mut revisioned = packages.clone();
    let mut metadata = registry_metadata("a");
    metadata.version = Some("9.9.9".to_string());
    revisioned.insert(key, metadata);
    assert_ne!(
        baseline,
        fingerprint(&snapshots, &revisioned, Some("linux;x64;22"), Some(&policy), Some(&dir)),
        "the version segment is part of the suffix, so it is part of the key",
    );

    let builds =
        crate::AllowBuildPolicy::new(HashSet::from(["bar".to_string()]), HashSet::new(), false);
    assert_ne!(
        baseline,
        fingerprint(&snapshots, &packages, Some("linux;x64;22"), Some(&builds), Some(&dir)),
        "the allow-build policy decides which snapshots carry the engine",
    );
    // No policy turns the gating off, which puts the engine in every
    // snapshot's hash — the opposite of what an empty policy does.
    assert_ne!(
        baseline,
        fingerprint(&snapshots, &packages, Some("linux;x64;22"), None, Some(&dir)),
        "an absent policy and one that allows nothing must not share an entry",
    );
}
#[test]
fn layout_cache_load_rejects_a_map_it_cannot_vouch_for() {
    let cache_dir = tempfile::tempdir().expect("create cache dir");
    let project = tempfile::tempdir().expect("create project dir");
    let file = |fingerprint| super::super::gvs_layout_cache::CacheFile {
        cache_dir: cache_dir.path(),
        lockfile_dir: project.path(),
        fingerprint,
    };
    let foo: PackageKey = "@scope/foo@1.2.3".parse().expect("parse key");
    let bar: PackageKey = "bar@4.5.6".parse().expect("parse key");
    let snapshots = HashMap::from([
        (foo.clone(), SnapshotEntry::default()),
        (bar.clone(), SnapshotEntry::default()),
    ]);
    let expected =
        super::super::gvs_layout_cache::Expected { snapshots: &snapshots, packages: None };
    let suffixes = HashMap::from([
        (foo.clone(), format!("@scope/foo/1.2.3/{DIGEST_A}")),
        (bar.clone(), format!("@/bar/4.5.6/{DIGEST_B}")),
    ]);

    super::super::gvs_layout_cache::store(file("fingerprint"), &suffixes);
    assert_eq!(
        super::super::gvs_layout_cache::load(file("fingerprint"), expected),
        Some(suffixes),
        "a stored map must load back unchanged",
    );
    assert_eq!(
        super::super::gvs_layout_cache::load(file("another-fingerprint"), expected),
        None,
        "an entry derived from other inputs must miss, not be reused",
    );

    let short = HashMap::from([(foo.clone(), format!("@scope/foo/1.2.3/{DIGEST_A}"))]);
    super::super::gvs_layout_cache::store(file("fingerprint"), &short);
    assert_eq!(
        super::super::gvs_layout_cache::load(file("fingerprint"), expected),
        None,
        "a map missing a snapshot must miss: the absent one would take the flat-name fallback",
    );

    let redirected = HashMap::from([
        (foo.clone(), format!("@/bar/4.5.6/{DIGEST_B}")),
        (bar.clone(), format!("@/bar/4.5.6/{DIGEST_B}")),
    ]);
    super::super::gvs_layout_cache::store(file("fingerprint"), &redirected);
    assert_eq!(
        super::super::gvs_layout_cache::load(file("fingerprint"), expected),
        None,
        "a suffix naming a different package must miss, however it got into the cache",
    );

    let traversing = HashMap::from([
        (
            "@scope/foo@1.2.3".parse::<PackageKey>().expect("parse key"),
            "@scope/foo/1.2.3/../../../../../../tmp/evil".to_string(),
        ),
        ("bar@4.5.6".parse::<PackageKey>().expect("parse key"), format!("@/bar/4.5.6/{DIGEST_B}")),
    ]);
    super::super::gvs_layout_cache::store(file("fingerprint"), &traversing);
    assert_eq!(
        super::super::gvs_layout_cache::load(file("fingerprint"), expected),
        None,
        "a suffix that walks back out of the store must miss: it names the right package, \
         and `join_global_virtual_store_path` would follow it anyway",
    );

    let downgraded = HashMap::from([
        (foo, format!("@scope/foo/9.9.9/{DIGEST_A}")),
        (bar, format!("@/bar/4.5.6/{DIGEST_B}")),
    ]);
    super::super::gvs_layout_cache::store(file("fingerprint"), &downgraded);
    assert_eq!(
        super::super::gvs_layout_cache::load(file("fingerprint"), expected),
        None,
        "a suffix naming another version of the package must miss too",
    );

    let stored: Vec<_> = std::fs::read_dir(cache_dir.path().join("gvs-layout"))
        .expect("read the cache dir")
        .map(|entry| entry.expect("cache entry").path())
        .collect();
    assert_eq!(
        stored.len(),
        1,
        "one project keeps one entry, however many times its inputs change",
    );

    std::fs::write(&stored[0], vec![0_u8; 8192]).expect("overwrite the entry with garbage");
    assert_eq!(
        super::super::gvs_layout_cache::load(file("fingerprint"), expected),
        None,
        "bytes that are not a map must miss rather than parse as an empty one",
    );
}
