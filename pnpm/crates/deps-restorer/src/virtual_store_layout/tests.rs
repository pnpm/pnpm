mod workspace;

mod reporting;

mod builds;

mod layout_cache;

mod slot_identity;

use super::VirtualStoreLayout;
use pnpm_config::Config;
use pnpm_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, PkgName, RegistryResolution, SnapshotDepRef,
    SnapshotEntry,
};
use serde::Deserialize;
use std::{collections::HashMap, path::PathBuf};

/// Build a `Config` test-double with the GVS-relevant fields
/// wired explicitly. `gvs_dir` populates `global_virtual_store_dir`
/// for the GVS-on path; `virtual_store_dir` stays at the
/// project-local default for the GVS-off path.
fn make_config(gvs: bool, virtual_store_dir: PathBuf, gvs_dir: PathBuf) -> Config {
    let mut config = Config::new();
    config.enable_global_virtual_store = gvs;
    config.virtual_store_dir = virtual_store_dir;
    config.global_virtual_store_dir = gvs_dir;
    config
}

/// [`PackageMetadata`] carrying only the fields the layout reads.
fn package_metadata(resolution: LockfileResolution, version: Option<&str>) -> PackageMetadata {
    PackageMetadata {
        resolution,
        version: version.map(str::to_string),
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

/// Build a registry snapshot carrying one `link:` dependency.
fn snapshot_with_link(alias: &str, target: &str) -> SnapshotEntry {
    let mut dependencies = HashMap::new();
    dependencies.insert(
        alias.parse::<PkgName>().unwrap(),
        format!("link:{target}").parse::<SnapshotDepRef>().unwrap(),
    );
    SnapshotEntry { dependencies: Some(dependencies), ..SnapshotEntry::default() }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinkHashParityFixture {
    package: LinkHashFixturePackage,
    posix: Vec<LinkHashFixtureCase>,
    win32: Vec<LinkHashFixtureCase>,
}

#[derive(Deserialize)]
struct LinkHashFixturePackage {
    key: String,
    name: String,
    version: String,
    alias: String,
    integrity: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinkHashFixtureCase {
    name: String,
    lockfile_dir: String,
    target: String,
    expected_link_node: String,
    expected_slot: String,
}

fn cyclic_slot_suffixes() -> Vec<(String, String)> {
    let config = make_config(
        true,
        PathBuf::from("/tmp/proj/node_modules/.pnpm"),
        PathBuf::from("/tmp/store/links"),
    );
    let (snapshots, packages) = cyclic_snapshots();
    // An empty policy allows no builds, which makes every snapshot hash
    // engine-agnostically — the digests below then hold on any host.
    let policy = crate::AllowBuildPolicy::default();
    let layout = VirtualStoreLayout::new(
        &config,
        Some("darwin-arm64-node20"),
        Some(&snapshots),
        Some(&packages),
        Some(&policy),
        None,
    );
    let mut suffixes: Vec<(String, String)> = snapshots
        .keys()
        .map(|snapshot_key| {
            let slot = layout.slot_dir(snapshot_key);
            let suffix = slot
                .strip_prefix("/tmp/store/links")
                .expect("slot under the global virtual store")
                .to_string_lossy()
                // `slot_dir` builds the tail from native components, so
                // compare against the `/`-separated form the GVS path is
                // canonically written in.
                .replace('\\', "/");
            (snapshot_key.to_string(), suffix)
        })
        .collect();
    suffixes.sort();
    suffixes
}

/// Two cyclic subgraphs whose digests only come out right when the
/// hasher visits a snapshot's children the way pnpm's
/// `{...dependencies, ...optionalDependencies}` object does.
///
/// `a` reaches the `p` ↔ `x` cycle through two regular dependencies —
/// aliased `c` and `p` — so it pins the order *within* a section.
/// `b` reaches the `n` ↔ `y` cycle through a regular `n` and an
/// optional `c`, whose alias sorts first, so it pins that
/// `optionalDependencies` still come last.
fn cyclic_snapshots() -> (HashMap<PackageKey, SnapshotEntry>, HashMap<PackageKey, PackageMetadata>)
{
    let snapshots = HashMap::from([
        (
            key("a@1.0.0"),
            SnapshotEntry {
                dependencies: Some(HashMap::from([
                    (alias("c"), SnapshotDepRef::Alias(key("q@1.0.0"))),
                    (alias("p"), plain("1.0.0")),
                ])),
                ..Default::default()
            },
        ),
        (
            key("b@1.0.0"),
            SnapshotEntry {
                dependencies: Some(HashMap::from([(alias("n"), plain("1.0.0"))])),
                optional_dependencies: Some(HashMap::from([(
                    alias("c"),
                    SnapshotDepRef::Alias(key("m@1.0.0")),
                )])),
                ..Default::default()
            },
        ),
        (key("m@1.0.0"), depends_on("y")),
        (key("n@1.0.0"), depends_on("y")),
        (key("p@1.0.0"), depends_on("x")),
        (key("q@1.0.0"), depends_on("x")),
        (key("x@1.0.0"), depends_on("p")),
        (key("y@1.0.0"), depends_on("n")),
    ]);
    let packages = snapshots
        .keys()
        .map(|snapshot_key| {
            let lead = snapshot_key.name.to_string().to_uppercase();
            (snapshot_key.clone(), registry_metadata(&lead))
        })
        .collect();
    (snapshots, packages)
}

fn key(text: &str) -> PackageKey {
    text.parse().expect("parse package key")
}

fn alias(text: &str) -> PkgName {
    PkgName::parse(text).expect("parse alias")
}

fn plain(version: &str) -> SnapshotDepRef {
    SnapshotDepRef::Plain(version.parse().expect("parse version"))
}

fn depends_on(name: &str) -> SnapshotEntry {
    SnapshotEntry {
        dependencies: Some(HashMap::from([(alias(name), plain("1.0.0"))])),
        ..Default::default()
    }
}

/// Registry metadata whose integrity starts with `lead`, so each
/// package contributes a distinct `full_pkg_id` to the digests.
fn registry_metadata(lead: &str) -> PackageMetadata {
    let integrity = format!("sha512-{lead}{}", "A".repeat(91));
    PackageMetadata {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: integrity.parse().expect("parse integrity"),
            revision: None,
        }),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

/// A cached suffix decides where a package is linked from, so the
/// loader has to distrust what it reads back. `cacheDir` is settable
/// from a repository's own `pnpm-workspace.yaml`, which makes the file
/// attacker-writable in a hostile checkout, and a write interrupted by
/// a crash leaves a shorter one.
/// A slot's last component is the hex `calc_graph_node_hash` produces.
const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
