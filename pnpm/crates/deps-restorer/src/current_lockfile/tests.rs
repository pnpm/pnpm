//! Unit tests for current-lockfile filtering and merge helpers.

mod lockfile;

mod links;

mod installation;

mod workspace;

use std::collections::{BTreeMap, HashMap};

use indexmap::IndexMap;
use pnpm_lockfile::{
    CatalogSnapshots, ComVer, ImporterDepVersion, Lockfile, LockfileResolution, LockfileSettings,
    LockfileVersion, PackageKey, PackageMetadata, PkgName, PkgVerPeer, ResolvedCatalogEntry,
    ResolvedDependencyMap, ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
    TarballResolution,
};
use pnpm_modules_yaml::IncludedDependencies;

fn pkg(name: &str) -> PkgName {
    PkgName::parse(name).expect("parse PkgName")
}

fn ver(text: &str) -> PkgVerPeer {
    text.parse().expect("parse PkgVerPeer")
}

fn key(name_text: &str, version: &str) -> PackageKey {
    PackageKey::new(pkg(name_text), ver(version))
}

fn importer_dep(version: &str) -> ResolvedDependencySpec {
    ResolvedDependencySpec {
        specifier: version.to_string(),
        version: ImporterDepVersion::Regular(ver(version)),
    }
}

fn importer_link(target: &str) -> ResolvedDependencySpec {
    ResolvedDependencySpec {
        specifier: "workspace:*".to_string(),
        version: ImporterDepVersion::Link(target.to_string()),
    }
}

fn importer_map(entries: &[(&str, &str)]) -> ResolvedDependencyMap {
    entries.iter().map(|(n, v)| (pkg(n), importer_dep(v))).collect()
}

fn snapshot_with_deps(deps: &[(&str, &str)]) -> SnapshotEntry {
    let map: HashMap<PkgName, SnapshotDepRef> =
        deps.iter().map(|(n, v)| (pkg(n), SnapshotDepRef::Plain(ver(v)))).collect();
    SnapshotEntry { dependencies: Some(map), ..Default::default() }
}

fn package_metadata(name: &str) -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Tarball(TarballResolution {
            integrity: None,
            tarball: format!("https://example.test/{name}.tgz"),
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
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

fn empty_lockfile() -> Lockfile {
    Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer { major: 9, minor: 0 }).unwrap(),
        settings: None,
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

fn include_all() -> IncludedDependencies {
    IncludedDependencies { dependencies: true, dev_dependencies: true, optional_dependencies: true }
}

fn lockfile_with_top_level(marker: &str, minor: u16) -> Lockfile {
    let catalogs = CatalogSnapshots::from([(
        "default".to_string(),
        BTreeMap::from([(
            "catalog-pkg".to_string(),
            ResolvedCatalogEntry {
                specifier: format!("{marker}-specifier"),
                version: format!("{marker}-version"),
            },
        )]),
    )]);
    Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer { major: 9, minor }).unwrap(),
        settings: Some(LockfileSettings {
            auto_install_peers: marker == "fresh",
            dedupe_peers: Some(marker == "fresh"),
            exclude_links_from_lockfile: marker != "fresh",
            inject_workspace_packages: marker == "fresh",
            peers_suffix_max_length: Some(if marker == "fresh" { 2000 } else { 1000 }),
        }),
        catalogs: Some(catalogs),
        overrides: Some(IndexMap::from([(
            "override-pkg".to_string(),
            format!("{marker}-override"),
        )])),
        package_extensions_checksum: Some(format!("{marker}-extensions")),
        pnpmfile_checksum: Some(format!("{marker}-pnpmfile")),
        ignored_optional_dependencies: Some(vec![format!("{marker}-ignored")]),
        patched_dependencies: Some(BTreeMap::from([(
            "patched@1.0.0".to_string(),
            format!("{marker}-patch"),
        )])),
        importers: HashMap::new(),
        packages: None,
        snapshots: None,
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    }
}
