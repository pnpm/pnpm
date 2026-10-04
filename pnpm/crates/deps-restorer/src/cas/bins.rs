use super::{LOADER_FILENAME, StoreManifest, write_file};
use pnpm_cmd_shim::{
    Host, PackageBinSource, get_bins_from_package_manifest, link_bins_of_packages,
};
use sha2::{Digest, Sha256};
use std::{io, path::Path, sync::Arc};
use url::Url;

pub(super) struct BinInstall<'a> {
    pub config: &'a pnpm_config::Config,
    pub root: &'a Path,
    pub importers: &'a std::collections::HashMap<String, pnpm_lockfile::ProjectSnapshot>,
}

pub(super) fn write_bins(inputs: &BinInstall<'_>, manifest: &StoreManifest) -> io::Result<()> {
    for importer in inputs.importers.keys() {
        let Some(project) = manifest.packages.get(importer) else { continue };
        let mut sources = Vec::new();
        for id in project.dependencies.values() {
            if let Some(source) = bin_source(inputs, manifest, id)? {
                sources.push(source);
            }
        }
        let directory = inputs.root
            .join(importer)
            .join(inputs.config.modules_dir_name())
            .join(".bin");
        link_bins_of_packages::<Host>(
            &sources,
            &directory,
            &crate::shim_link_options(inputs.config, pnpm_config::NodeLinker::Cas),
        )
        .map_err(io::Error::other)?;
        super::bin_state::reconcile_bins(&sources, &directory)?;
    }
    Ok(())
}

fn bin_source(
    inputs: &BinInstall<'_>,
    manifest: &StoreManifest,
    id: &str,
) -> io::Result<Option<PackageBinSource>> {
    let Some(package) = manifest.packages.get(id) else { return Ok(None) };
    let hash = format!("{:x}", Sha256::digest(id.as_bytes()));
    let root = package.root
        .clone()
        .unwrap_or_else(|| inputs.root.join(".pnpm-loader").join(&hash));
    let metadata = read_manifest(package, &manifest.store_dir, &root)?;
    if package.resolution.as_deref() == Some("node") {
        return Ok(Some(PackageBinSource::new(root, Arc::new(metadata))));
    }
    let commands = get_bins_from_package_manifest::<Host>(&metadata, &root);
    if commands.is_empty() {
        return Ok(None);
    }
    let entry_dir = inputs.config.install_state_dir.join("cas-bins").join(hash);
    std::fs::create_dir_all(&entry_dir)?;
    let mut metadata = metadata;
    let mut bins = serde_json::Map::new();
    for (index, command) in commands.iter().enumerate() {
        let filename = format!("{index}.mjs");
        write_entry(inputs.root, &entry_dir.join(&filename), &command.path)?;
        bins.insert(command.name.clone(), filename.into());
    }
    metadata["bin"] = bins.into();
    Ok(Some(PackageBinSource::new(entry_dir, Arc::new(metadata))))
}

fn write_entry(root: &Path, path: &Path, target: &Path) -> io::Result<()> {
    let loader = file_url(&root.join(LOADER_FILENAME))?;
    let target = serde_json::to_string(target)?;
    let loader = serde_json::to_string(&loader)?;
    let source = format!(
        "#!/usr/bin/env node\nimport {loader};\nimport {{ runMain }} from 'node:module';\nprocess.argv[1] = {target};\nprocess.env.NODE_OPTIONS = (process.env.NODE_OPTIONS ?? '') + ' --import=' + JSON.stringify({loader});\nrunMain({target});\n",
    );
    write_file(path, source.as_bytes())
}

fn file_url(path: &Path) -> io::Result<String> {
    Url::from_file_path(path)
        .map(String::from)
        .map_err(|()| io::Error::other(format!("Invalid file path: {}", path.display())))
}

fn read_manifest(
    package: &super::StorePackage,
    store: &Path,
    root: &Path,
) -> io::Result<serde_json::Value> {
    let path = match &package.files {
        Some(files) => {
            let Some(hash) = files.get("package.json") else {
                return Ok(serde_json::json!({}));
            };
            store
                .join("files")
                .join(&hash[..2])
                .join(&hash[2..])
        }
        None => root.join("package.json"),
    };
    pnpm_package_manifest::parse_manifest_bytes(&std::fs::read(path)?).map_err(io::Error::other)
}
