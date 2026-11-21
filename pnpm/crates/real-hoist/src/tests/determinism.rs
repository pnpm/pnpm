//! The layout is a function of the lockfile alone. A lockfile is read into
//! hash maps whose iteration order differs from one map to the next, and
//! the hoister keys its own state by node address. Neither may reach the
//! result: the same lockfile gives the same directories in the same order,
//! each serving the same snapshot keys in the same order.

use super::{
    HoistOpts,
    HoisterResult,
    Lockfile,
};
use crate::{
    hoist_until_peers_resolve,
    tree,
};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::Path,
};

/// A linear congruential generator, so the lockfile is the same on every
/// run and platform.
struct Rng(u64);

impl Rng {
    fn below(&mut self, bound: u64) -> u64 {
        self.0 =
            self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % bound
    }
}

const PACKAGES: u64 = 40;
const PROVIDERS: u64 = 4;
const PROJECTS: u64 = 6;

/// Every fifth package takes `provider{n}` as a peer.
fn peer_of(package: u64) -> Option<u64> {
    package
        .is_multiple_of(5)
        .then_some(package / 5 % PROVIDERS)
}

type Dependencies = BTreeMap<String, String>;

/// The dependencies of one dependent: random packages above `floor`, each
/// peer bound to the provider version the dependent picked, and that
/// provider itself. `own_peer` is the provider and version the dependent
/// is bound to as a peer of its own.
fn dependencies(
    rng: &mut Rng,
    floor: u64,
    count: u64,
    own_peer: Option<(u64, u64)>,
) -> Dependencies {
    let mut provider_versions: Vec<u64> = (0..PROVIDERS)
        .map(|_| 1 + rng.below(2))
        .collect();
    let mut lines = Dependencies::new();
    if let Some((provider, version)) = own_peer {
        provider_versions[provider as usize] = version;
        lines.insert(format!("provider{provider}"), format!("{version}.0.0"));
    }
    let above = PACKAGES.saturating_sub(floor + 1);
    for _ in 0..count.min(above) {
        let package = floor + 1 + rng.below(above);
        let mut reference = format!("{}.0.0", 1 + rng.below(2));
        if let Some(provider) = peer_of(package) {
            let provider_version = provider_versions[provider as usize];
            write!(reference, "(provider{provider}@{provider_version}.0.0)").unwrap();
            lines.insert(format!("provider{provider}"), format!("{provider_version}.0.0"));
        }
        lines.insert(format!("package{package:02}"), reference);
    }
    lines
}

fn importer_yaml(rng: &mut Rng, project: u64) -> String {
    let id = if project == 0 { ".".to_string() } else { format!("packages/project{project}") };
    let mut yaml = format!("  {id}:\n    dependencies:\n");
    for (name, reference) in dependencies(rng, 0, 9, None) {
        writeln!(yaml, "      {name}: {{specifier: '*', version: '{reference}'}}").unwrap();
    }
    yaml
}

fn snapshot_yaml(rng: &mut Rng, key: &str, package: u64, own_peer: Option<(u64, u64)>) -> String {
    let suffix = own_peer
        .map(|(provider, bound)| format!("(provider{provider}@{bound}.0.0)"))
        .unwrap_or_default();
    let own = dependencies(rng, package, 3, own_peer);
    if own.is_empty() {
        return format!("  {key}{suffix}: {{}}\n");
    }
    let mut yaml = format!("  {key}{suffix}:\n    dependencies:\n");
    for (name, reference) in own {
        writeln!(yaml, "      {name}: '{reference}'").unwrap();
    }
    yaml
}

/// The `packages:` entry of one package version, and its `snapshots:`
/// entries: one per provider version when it takes a peer.
fn package_yaml(rng: &mut Rng, package: u64, version: u64) -> (String, String) {
    let key = format!("package{package:02}@{version}.0.0");
    let peer = peer_of(package);
    let peers = peer
        .map(|provider| format!(", peerDependencies: {{provider{provider}: '*'}}"))
        .unwrap_or_default();
    let entry = format!("  {key}: {{resolution: {{integrity: sha512-{key}}}{peers}}}\n");
    let bindings: Vec<Option<(u64, u64)>> = match peer {
        Some(provider) => vec![Some((provider, 1)), Some((provider, 2))],
        None => vec![None],
    };
    // Every peer variant draws from the same state: a package version has
    // one set of dependencies, and only the provider it is bound to differs.
    let state = rng.0;
    let snapshots = bindings
        .into_iter()
        .map(|own_peer| {
            rng.0 = state;
            snapshot_yaml(rng, &key, package, own_peer)
        })
        .collect();
    (entry, snapshots)
}

/// A workspace of [`PROJECTS`] projects over [`PACKAGES`] packages in two
/// versions each, with shared dependencies, version conflicts and peers.
fn lockfile_yaml() -> String {
    let mut rng = Rng(18_303);
    let mut yaml = String::from("lockfileVersion: '9.0'\nimporters:\n");
    for project in 0..PROJECTS {
        yaml.push_str(&importer_yaml(&mut rng, project));
    }
    yaml.push_str("packages:\n");
    let mut snapshots = String::from("snapshots:\n");
    let provider_keys = (0..PROVIDERS * 2).map(|n| format!("provider{}@{}.0.0", n / 2, 1 + n % 2));
    for key in provider_keys {
        writeln!(yaml, "  {key}: {{resolution: {{integrity: sha512-{key}}}}}").unwrap();
        writeln!(snapshots, "  {key}: {{}}").unwrap();
    }
    for n in 0..PACKAGES * 2 {
        let (entry, own_snapshots) = package_yaml(&mut rng, n / 2, 1 + n % 2);
        yaml.push_str(&entry);
        snapshots.push_str(&own_snapshots);
    }
    yaml.push_str(&snapshots);
    yaml
}

/// Every directory in result order with the snapshot keys it serves.
fn layout(node: &HoisterResult, prefix: &str, out: &mut String) {
    for dep in node.dependencies.borrow().iter() {
        let path = format!("{prefix}/{}", dep.0.name);
        let references: Vec<String> = dep.0.references
            .borrow()
            .iter()
            .cloned()
            .collect();
        writeln!(out, "{path}\t{}", references.join(" ")).unwrap();
        layout(&dep.0, &path, out);
    }
}

fn hoisted_layout(yaml: &str) -> String {
    let lockfile = Lockfile::parse(yaml, Path::new("pnpm-lock.yaml"))
        .expect("parse the lockfile")
        .expect("the lockfile is not empty");
    // Not through `hoist`, whose resolution oracle rejects a package version
    // reached through another peer variant. Those layouts must repeat too.
    let (input, result) =
        hoist_until_peers_resolve(&lockfile, &HoistOpts::default()).expect("hoist should succeed");
    tree::tear_down(&input);
    let mut out = String::new();
    layout(&result, "", &mut out);
    out
}

#[test]
fn the_same_lockfile_gives_the_same_layout_every_time() {
    let yaml = lockfile_yaml();
    let first = hoisted_layout(&yaml);
    let nested = first
        .lines()
        .filter(|line| line.matches('/').count() >= 3)
        .count();
    assert!(nested > 20, "the layout nests conflicting versions: {first}");
    for attempt in 1..8 {
        // Each parse fills fresh hash maps, with an iteration order of their own.
        let again = hoisted_layout(&yaml);
        assert!(
            again == first,
            "attempt {attempt} gave another layout:\n{again}\n\nfirst:\n{first}",
        );
    }
}

/// `m`'s key names its transitive peer's provider as `f@2.0.0`, and its
/// dependencies record two full keys for it. Which dependency is read
/// first depends on the hash map, so the smaller key is taken.
#[test]
fn a_provider_recorded_under_several_keys_resolves_to_the_same_one() {
    let yaml = "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      m: {specifier: 1.0.0, version: 1.0.0(f@2.0.0)}
packages:
  c@1.0.0: {resolution: {integrity: sha512-c}, peerDependencies: {f: '*'}}
  d@1.0.0: {resolution: {integrity: sha512-d}, peerDependencies: {f: '*'}}
  f@2.0.0: {resolution: {integrity: sha512-f}, peerDependencies: {r: '*'}}
  m@1.0.0: {resolution: {integrity: sha512-m}}
  r@1.0.0: {resolution: {integrity: sha512-r1}}
  r@2.0.0: {resolution: {integrity: sha512-r2}}
snapshots:
  c@1.0.0(f@2.0.0):
    dependencies: {f: 2.0.0(r@2.0.0)}
  d@1.0.0(f@2.0.0):
    dependencies: {f: 2.0.0(r@1.0.0)}
  f@2.0.0(r@1.0.0):
    dependencies: {r: 1.0.0}
  f@2.0.0(r@2.0.0):
    dependencies: {r: 2.0.0}
  m@1.0.0(f@2.0.0):
    dependencies: {c: 1.0.0(f@2.0.0), d: 1.0.0(f@2.0.0)}
    transitivePeerDependencies: [f]
  r@1.0.0: {}
  r@2.0.0: {}
";
    for attempt in 0..16 {
        let lockfile = Lockfile::parse(yaml, Path::new("pnpm-lock.yaml"))
            .expect("parse the lockfile")
            .expect("the lockfile is not empty");
        let key = "m@1.0.0(f@2.0.0)".parse().expect("parse the key");
        let snapshot = &lockfile.snapshots.as_ref().expect("snapshots")[&key];
        let provider = tree::peers::provider_of(&key, snapshot, "f", &lockfile)
            .map(|provider| provider.to_string());
        assert_eq!(provider.as_deref(), Some("f@2.0.0(r@1.0.0)"), "attempt {attempt}");
    }
}
