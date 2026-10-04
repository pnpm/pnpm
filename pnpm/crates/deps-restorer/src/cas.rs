pub(crate) use link::link_phase;

use crate::{
    SkippedSnapshots,
    linking::LinkPhaseInputs,
    package_map::{PackageMapOptions, lockfile_to_package_map},
};
use pnpm_lockfile::PkgIdWithPatchHash;
use std::{collections::BTreeMap, io, path::Path};
use url::Url;

use pnpm_config::{
    CAS_LOADER_FILENAME as LOADER_FILENAME, CAS_MANIFEST_FILENAME as MANIFEST_FILENAME,
    NodePackageMapType,
};
use pnpm_store_dir::{StoreLoaderManifest as StoreManifest, StoreLoaderPackage as StorePackage};

mod bin_state;
mod bins;
mod link;

const LOADER: &str = include_str!("cas-loader.mjs.inc");

pub(crate) fn write_installation(
    inputs: &LinkPhaseInputs<'_>,
    skipped: &SkippedSnapshots,
) -> io::Result<()> {
    let manifest = build_manifest(inputs, skipped)?;
    let root = inputs.ctx.workspace_root;
    write_file(&root.join(MANIFEST_FILENAME), &serde_json::to_vec(&manifest)?)?;
    write_file(&root.join(LOADER_FILENAME), LOADER.as_bytes())?;
    bins::write_bins(
        &bins::BinInstall {
            config: inputs.ctx.config,
            root,
            importers: &inputs.graph.lockfile.importers,
        },
        &manifest,
    )
}

fn build_manifest(
    inputs: &LinkPhaseInputs<'_>,
    skipped: &SkippedSnapshots,
) -> io::Result<StoreManifest> {
    let config = inputs.ctx.config;
    let lockfile = inputs.graph.lockfile;
    let selected = crate::create_virtual_store::cas::materialized_snapshots(
        config,
        lockfile.snapshots.as_ref(),
    );
    let base = Url::from_directory_path(&config.modules_dir)
        .map_err(|()| io::Error::other("CAS modules directory must be absolute"))?;
    let snapshots: std::collections::HashMap<_, _> = lockfile.snapshots
        .iter()
        .flatten()
        .map(|(key, _)| (key.to_string(), key))
        .collect();
    let mut packages = BTreeMap::new();
    for (id, package) in package_map(inputs).packages {
        let snapshot = snapshots.get(&id).copied();
        if snapshot.is_some_and(|key| skipped.contains(key)) {
            continue;
        }
        let entry = store_package(
            inputs,
            (&id, package),
            (&base, snapshot.map(|key| selected.contains_key(key))),
        )?;
        packages.insert(id, entry);
    }
    let ids: std::collections::HashSet<_> = packages.keys().cloned().collect();
    for package in packages.values_mut() {
        package.dependencies.retain(|_, id| ids.contains(id));
    }
    Ok(StoreManifest { version: 1, store_dir: config.store_dir.root().to_path_buf(), packages })
}

fn package_map(inputs: &LinkPhaseInputs<'_>) -> crate::package_map::PackageMap {
    let config = inputs.ctx.config;
    let lockfile = inputs.graph.lockfile;
    lockfile_to_package_map(
        lockfile,
        &PackageMapOptions {
            lockfile_dir: inputs.ctx.workspace_root,
            modules_dir: &config.modules_dir,
            package_map_type: NodePackageMapType::Standard,
            layout: inputs.ctx.linker.layout,
            project_manifests: inputs.projects.manifests,
        },
    )
}

fn store_package(
    inputs: &LinkPhaseInputs<'_>,
    (id, package): (&str, crate::package_map::PackageMapPackage),
    (base, materialized): (&Url, Option<bool>),
) -> io::Result<StorePackage> {
    let root = base
        .join(&package.url)
        .map_err(io::Error::other)?
        .to_file_path()
        .map_err(|()| io::Error::other("Invalid package directory URL"))?;
    let mut entry = StorePackage {
        root: Some(root),
        files: None,
        resolution: None,
        dependencies: package.dependencies,
    };
    if materialized == Some(true) {
        entry.resolution = Some("node".to_string());
        entry.dependencies.clear();
    } else if materialized == Some(false) {
        entry.files = Some(package_files(inputs, id)?);
        entry.root = None;
    }
    Ok(entry)
}

fn package_files(inputs: &LinkPhaseInputs<'_>, id: &str) -> io::Result<BTreeMap<String, String>> {
    let key = PkgIdWithPatchHash::from(pnpm_deps_path::get_pkg_id_with_patch_hash(id).to_string());
    let paths = inputs.packages.cas_paths_by_pkg_id
        .as_ref()
        .and_then(|paths| paths.get(&key))
        .ok_or_else(|| io::Error::other(format!("Missing CAS files for {id}")))?;
    let files_dir = inputs.ctx.config.store_dir.root().join("files");
    paths.cas_paths
        .iter()
        .map(|(name, path)| {
            let relative = path
                .strip_prefix(&files_dir)
                .map_err(|_| {
                    io::Error::other(format!("{id} has non-CAS files; add it to casMaterialize"))
                })?;
            let hash = relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<String>();
            Ok((name.clone(), hash))
        })
        .collect()
}

fn write_file(path: &Path, contents: &[u8]) -> io::Result<()> {
    pnpm_fs::ensure_file(path, contents, None).map_err(io::Error::other)
}

pub(crate) fn refresh_bins(
    config: &pnpm_config::Config,
    root: &Path,
    importers: &std::collections::HashMap<String, pnpm_lockfile::ProjectSnapshot>,
) -> io::Result<()> {
    let manifest = serde_json::from_slice(&std::fs::read(root.join(MANIFEST_FILENAME))?)?;
    bins::write_bins(&bins::BinInstall { config, root, importers }, &manifest)
}
