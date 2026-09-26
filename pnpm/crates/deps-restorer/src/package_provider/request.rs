use super::{
    PackageProviderError, PackageProviderInputs, ProviderGitSource, ProviderPatch, ProviderRequest,
    ProviderRequestBundle, ProviderRequestDep, ProviderRequestNode, types::PROTOCOL_VERSION,
};
use crate::install_package_by_snapshot::tarball_url_and_integrity;
use pnpm_lockfile::{LockfileResolution, PackageKey, PackageMetadata, SnapshotEntry, VersionPart};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
};

/// Build the protocol-v1 request from the dependency graph.
pub(crate) fn build_provider_request(
    inputs: &PackageProviderInputs<'_>,
) -> Result<Option<ProviderRequestBundle>, PackageProviderError> {
    let (Some(snapshots), Some(packages)) = (inputs.snapshots, inputs.packages) else {
        return Ok(None);
    };
    let engine =
        inputs.engine.map_or_else(|| pnpm_graph_hasher::engine_name(0, None, None), str::to_string);

    let mut nodes: BTreeMap<String, ProviderRequestNode> = BTreeMap::new();
    let mut key_by_dep_path: HashMap<String, PackageKey> = HashMap::new();
    for (key, snapshot) in snapshots {
        if inputs.skipped.contains(key) {
            continue;
        }
        let dep_path = key.to_string();
        let node = build_node(key, snapshot, &dep_path, inputs, packages, snapshots, &engine)?;
        key_by_dep_path.insert(dep_path.clone(), key.clone());
        nodes.insert(dep_path, node);
    }

    if nodes.is_empty() {
        return Ok(None);
    }

    let gc_root_dir = inputs.lockfile_dir
        .join("node_modules")
        .join(".pnpm-nix")
        .to_string_lossy()
        .into_owned();
    Ok(Some(ProviderRequestBundle {
        request: ProviderRequest { protocol: PROTOCOL_VERSION, gc_root_dir, nodes },
        key_by_dep_path,
    }))
}

fn build_node(
    key: &PackageKey,
    snapshot: &SnapshotEntry,
    dep_path: &str,
    inputs: &PackageProviderInputs<'_>,
    packages: &HashMap<PackageKey, PackageMetadata>,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    engine: &str,
) -> Result<ProviderRequestNode, PackageProviderError> {
    let metadata_key = key.without_peer();
    let Some(metadata) = packages.get(&metadata_key) else {
        return Err(PackageProviderError::MissingPackageMetadata {
            dep_path: dep_path.to_string(),
        });
    };

    let name = key.name.to_string();
    let version = metadata.version
        .clone()
        .or_else(|| match key.suffix.version() {
            VersionPart::Semver(_) => Some(key.suffix.version().to_string()),
            VersionPart::RegistryQualified { version, .. } => Some(version.to_string()),
            VersionPart::File(_) | VersionPart::NonSemver(_) => None,
        });

    let mut node = ProviderRequestNode {
        name: name.clone(),
        version,
        tarball: None,
        integrity: None,
        directory: None,
        git: None,
        deps: BTreeMap::new(),
        engine: engine.to_string(),
        optional: snapshot.optional.then_some(true),
        patch: None,
    };

    populate_resolution(&mut node, metadata, dep_path, key, inputs)?;
    populate_patch(&mut node, &metadata_key, dep_path, inputs)?;
    populate_deps(&mut node, snapshot, &name, dep_path, inputs, snapshots)?;
    Ok(node)
}

fn populate_resolution(
    node: &mut ProviderRequestNode,
    metadata: &PackageMetadata,
    dep_path: &str,
    key: &PackageKey,
    inputs: &PackageProviderInputs<'_>,
) -> Result<(), PackageProviderError> {
    match &metadata.resolution {
        LockfileResolution::Tarball(_) | LockfileResolution::Registry(_) => {
            let unsupported = || PackageProviderError::UnsupportedResolution {
                dep_path: dep_path.to_string(),
                kind: "tarball without integrity",
            };
            let (tarball, integrity) =
                tarball_url_and_integrity(&metadata.resolution, key, inputs.config)
                    .map_err(|_| unsupported())?;
            let integrity = integrity.ok_or_else(unsupported)?;
            node.tarball = Some(tarball.into_owned());
            node.integrity = Some(integrity.to_string());
        }
        LockfileResolution::Directory(dir_resolution) => {
            let directory = Path::new(&dir_resolution.directory);
            let directory = if directory.is_absolute() {
                directory.to_path_buf()
            } else {
                inputs.lockfile_dir.join(directory)
            };
            node.directory =
                Some(pnpm_fs::lexical_normalize(&directory).to_string_lossy().into_owned());
        }
        LockfileResolution::Git(git_resolution) => {
            if metadata.prepare == Some(true) {
                return Err(PackageProviderError::GitPrepareUnsupported {
                    dep_path: dep_path.to_string(),
                });
            }
            node.git = Some(ProviderGitSource {
                repo: git_resolution.repo.clone(),
                commit: git_resolution.commit.clone(),
            });
        }
        LockfileResolution::Binary(_) => {
            return Err(PackageProviderError::UnsupportedResolution {
                dep_path: dep_path.to_string(),
                kind: "binary",
            });
        }
        LockfileResolution::Variations(_) => {
            return Err(PackageProviderError::UnsupportedResolution {
                dep_path: dep_path.to_string(),
                kind: "variations",
            });
        }
        LockfileResolution::Custom(_) => {
            return Err(PackageProviderError::UnsupportedResolution {
                dep_path: dep_path.to_string(),
                kind: "custom",
            });
        }
    }
    Ok(())
}

fn populate_patch(
    node: &mut ProviderRequestNode,
    metadata_key: &PackageKey,
    dep_path: &str,
    inputs: &PackageProviderInputs<'_>,
) -> Result<(), PackageProviderError> {
    if let Some(patch_info) = inputs.patches.and_then(|patches| patches.get(metadata_key)) {
        let Some(patch_file_path) = &patch_info.patch_file_path else {
            return Err(PackageProviderError::PatchWithoutFile { dep_path: dep_path.to_string() });
        };
        let content = std::fs::read_to_string(patch_file_path)
            .map_err(|source| PackageProviderError::ReadPatchFile {
                dep_path: dep_path.to_string(),
                path: patch_file_path.clone(),
                source,
            })?;
        node.patch = Some(ProviderPatch { content, hash: patch_info.hash.clone() });
    }
    Ok(())
}

fn populate_deps(
    node: &mut ProviderRequestNode,
    snapshot: &SnapshotEntry,
    name: &str,
    dep_path: &str,
    inputs: &PackageProviderInputs<'_>,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
) -> Result<(), PackageProviderError> {
    for dep_map in [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()] {
        let Some(dep_map) = dep_map else { continue };
        for (alias, dep_ref) in dep_map {
            let Some(resolved) = dep_ref.resolve(alias) else { continue };
            if inputs.skipped.contains(&resolved) || !snapshots.contains_key(&resolved) {
                continue;
            }
            let alias = alias.to_string();
            if alias == name {
                return Err(PackageProviderError::SelfDependency {
                    dep_path: dep_path.to_string(),
                });
            }
            node.deps.insert(
                alias,
                ProviderRequestDep {
                    dep_path: resolved.to_string(),
                    name: resolved.name.to_string(),
                },
            );
        }
    }
    Ok(())
}
