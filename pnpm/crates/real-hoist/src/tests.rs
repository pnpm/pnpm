use super::{
    HoistError, HoistOpts, HoisterResult, RcByPtr, build_hoist_ident_map, hoist,
    is_preferred_ident, percent_encode_path,
};
use indexmap::IndexSet;
use pnpm_lockfile::{
    ComVer, Lockfile, LockfileSettings, LockfileVersion, PkgName, PkgNameVerPeer, PkgVerPeer,
    ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
};
use pretty_assertions::assert_eq;
use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap, VecDeque},
    rc::Rc,
};

fn lockfile_version() -> LockfileVersion<9> {
    LockfileVersion::<9>::try_from(ComVer::new(9, 0)).expect("lockfileVersion 9.0 is compatible")
}

fn pkg_name(name: &str) -> PkgName {
    PkgName::parse(name).expect("parse PkgName")
}

fn ver_peer(spec: &str) -> PkgVerPeer {
    spec.parse::<PkgVerPeer>().expect("parse PkgVerPeer")
}

fn dep_key(name: &str, version: &str) -> PkgNameVerPeer {
    PkgNameVerPeer::new(pkg_name(name), ver_peer(version))
}

fn resolved_dep(version: &str) -> ResolvedDependencySpec {
    ResolvedDependencySpec { specifier: version.to_string(), version: ver_peer(version).into() }
}

fn empty_lockfile() -> Lockfile {
    Lockfile {
        lockfile_version: lockfile_version(),
        settings: Some(LockfileSettings::default()),
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers: HashMap::new(),
        packages: None,
        snapshots: None,
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    }
}

/// Helper for the peer-aware tests: build a `PackageMetadata`
/// whose `packages:`-level `peer_dependencies` claims one peer.
fn pkg_metadata_with_peer(peer_name: &str) -> pnpm_lockfile::PackageMetadata {
    use pnpm_lockfile::{LockfileResolution, PackageMetadata, TarballResolution};
    let mut peer_deps = HashMap::new();
    peer_deps.insert(peer_name.to_string(), "*".to_string());
    PackageMetadata {
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: format!("https://example.invalid/{peer_name}-host.tgz"),
            integrity: None,
            revision: None,
            git_hosted: None,
            path: None,
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
        peer_dependencies: Some(peer_deps),
        peer_dependencies_meta: None,
    }
}

/// Construct a [`HoisterResult`] node directly, for unit-testing the
/// preference machinery without going through [`hoist`] (which only ever
/// builds the `.` root with empty `peer_names`).
fn result_node(
    name: &str,
    reference: &str,
    peer_names: &[&str],
    dependencies: Vec<Rc<HoisterResult>>,
) -> Rc<HoisterResult> {
    Rc::new(HoisterResult {
        name: name.to_string(),
        ident_name: name.to_string(),
        references: RefCell::new(BTreeSet::from([reference.to_string()])),
        peer_names: peer_names.iter().map(|&peer| peer.to_string()).collect(),
        dependencies: RefCell::new(dependencies.into_iter().map(RcByPtr).collect::<IndexSet<_>>()),
        hoisted_dependencies: RefCell::new(std::collections::HashMap::new()),
        decoupled: std::cell::Cell::new(false),
    })
}

mod workspace_settings_hoist_throws_on_broken;

mod workspace_settings_multi_importer_lockfile_emits;

mod lockfile;

mod dependencies;

mod behavior;
