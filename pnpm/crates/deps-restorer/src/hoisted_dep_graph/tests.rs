mod resolution;

mod links;

mod workspace;

mod reporting;

mod lockfile;

mod runtimes;

mod installation;

use super::LockfileToHoistedDepGraphOptions;
use pnpm_lockfile::{DirectoryResolution, LockfileResolution};
use std::path::PathBuf;

fn sample_resolution() -> LockfileResolution {
    DirectoryResolution { directory: "../local-pkg".to_string() }.into()
}

/// Sample v9 depPath. v9 lockfiles use `name@version[(peers)]`
/// (see `PkgNameVerPeer` in `pnpm-lockfile`); the v5-era
/// `/name/version` shape is only kept for legacy
/// `hoistedAliases` read-side compatibility.
const ACCEPTS_DEP_PATH: &str = "accepts@1.3.7";

// --- Walker tests ----------------------------------------------------

use pnpm_lockfile::{
    ComVer, Lockfile, LockfileSettings, LockfileVersion, PackageKey, PackageMetadata, PkgName,
    PkgNameVerPeer, PkgVerPeer, ProjectSnapshot, ResolvedDependencyMap, ResolvedDependencySpec,
    SnapshotEntry,
};
use std::collections::HashMap;

fn lockfile_version() -> LockfileVersion<9> {
    LockfileVersion::<9>::try_from(ComVer::new(9, 0)).expect("lockfileVersion 9.0 is compatible")
}

fn pkg_name(text: &str) -> PkgName {
    PkgName::parse(text).expect("parse PkgName")
}

fn ver_peer(text: &str) -> PkgVerPeer {
    text.parse::<PkgVerPeer>().expect("parse PkgVerPeer")
}

fn dep_key(name: &str, version: &str) -> PkgNameVerPeer {
    PkgNameVerPeer::new(pkg_name(name), ver_peer(version))
}

fn resolved_dep(version: &str) -> ResolvedDependencySpec {
    ResolvedDependencySpec { specifier: version.to_string(), version: ver_peer(version).into() }
}

fn directory_resolution(directory: &str) -> LockfileResolution {
    DirectoryResolution { directory: directory.to_string() }.into()
}

/// Uses a synthetic `directory:` resolution: walker tests don't exercise
/// resolution semantics — they only need *some* resolution so the graph node
/// has a non-default value to inspect.
fn metadata_stub() -> PackageMetadata {
    PackageMetadata {
        resolution: directory_resolution("/dev/null/stub"),
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

fn lockfile_with(
    importer_deps: ResolvedDependencyMap,
    packages: HashMap<PackageKey, PackageMetadata>,
    snapshots: HashMap<PackageKey, SnapshotEntry>,
) -> Lockfile {
    let mut importers = HashMap::new();
    importers.insert(
        Lockfile::ROOT_IMPORTER_KEY.to_string(),
        ProjectSnapshot { dependencies: Some(importer_deps), ..ProjectSnapshot::default() },
    );
    Lockfile {
        lockfile_version: lockfile_version(),
        settings: Some(LockfileSettings::default()),
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers,
        packages: Some(packages),
        snapshots: Some(snapshots),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    }
}

// --- Installability tests --------------------------------------------

fn host_aware_opts() -> LockfileToHoistedDepGraphOptions<'static> {
    // Concrete platform values so the installability check has
    // something to compare against. The specific host doesn't
    // matter — tests assert relative behavior (compatible vs
    // incompatible) by setting metadata that targets *this*
    // value or its opposite.
    LockfileToHoistedDepGraphOptions {
        lockfile_dir: PathBuf::from("/repo"),
        current_node_version: "20.0.0".to_string(),
        current_os: "linux".to_string(),
        current_cpu: "x64".to_string(),
        current_libc: "glibc".to_string(),
        ..LockfileToHoistedDepGraphOptions::default()
    }
}

fn metadata_with_os(os: &str) -> PackageMetadata {
    PackageMetadata { os: Some(vec![os.to_string()]), ..metadata_stub() }
}

// --- Multi-importer (workspace) walker tests --------------------------

/// Build a multi-importer workspace fixture lockfile. Each
/// importer in `importer_deps` becomes a `ProjectSnapshot`
/// with the supplied direct deps. Root importer (`.`) takes
/// the first entry in `importer_deps`; remaining entries
/// become non-root workspace importers under their lockfile
/// keys.
fn workspace_lockfile(
    importer_deps: Vec<(&str, ResolvedDependencyMap)>,
    packages: HashMap<PackageKey, PackageMetadata>,
    snapshots: HashMap<PackageKey, SnapshotEntry>,
) -> Lockfile {
    let mut importers = HashMap::new();
    for (id, deps) in importer_deps {
        importers.insert(
            id.to_string(),
            ProjectSnapshot { dependencies: Some(deps), ..ProjectSnapshot::default() },
        );
    }
    Lockfile {
        lockfile_version: lockfile_version(),
        settings: Some(LockfileSettings::default()),
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers,
        packages: Some(packages),
        snapshots: Some(snapshots),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    }
}
