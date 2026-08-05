//! External package-provider integration (the `packageProvider`
//! setting).
//!
//! When a provider executable is configured, the install sends the
//! whole resolved dependency graph to it as one JSON request on stdin
//! (protocol v1) and reads one JSON response from stdout. The provider
//! materializes every depPath as a read-only directory (e.g. a Nix
//! store path) whose `node_modules` holds the package next to symlinks
//! to its dependencies, with lifecycle scripts already run. The install
//! then symlinks each importer's direct dependencies straight to the
//! returned directories and skips virtual-store materialization,
//! per-slot bin linking, hoisting, and the dependency build phase.
//!
//! Mirrors `materializeThroughPackageProvider` in
//! `@pnpm/installing.deps-restorer` — the request/response shape, the
//! validation rules, and every error message are shared contract
//! between the two stacks.

use crate::{SkippedSnapshots, install_package_by_snapshot::tarball_url_and_integrity};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, SnapshotEntry, VersionPart,
};
use pacquet_patching::ExtendedPatchInfo;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const PROTOCOL_VERSION: u32 = 1;

/// Everything the provider request is built from. Both install paths
/// (frozen and fresh) have these on hand once the dependency graph is
/// known: the lockfile-shaped `snapshots:` / `packages:` maps, the
/// install-time skip set, and the resolved patch info.
pub struct PackageProviderInputs<'a> {
    /// Path to the provider executable (the `packageProvider` setting).
    pub package_provider: &'a str,
    /// The directory containing `pnpm-lock.yaml`. Anchors the request's
    /// `gcRootDir` and the resolution of relative `directory:` paths.
    pub lockfile_dir: &'a Path,
    pub snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    pub packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    /// Snapshots excluded from this install (installability skips,
    /// `--no-optional` exclusions). They are omitted from the request
    /// and edges pointing at them are dropped, keeping the request a
    /// closed graph over materialized packages.
    pub skipped: &'a SkippedSnapshots,
    /// Patch info per peer-stripped package key, resolved from the
    /// `patchedDependencies` setting by the calling install path.
    pub patches: Option<&'a HashMap<PackageKey, ExtendedPatchInfo>>,
    /// The install-wide `ENGINE_NAME` string
    /// ([`pacquet_graph_hasher::engine_name`]) — the same value the
    /// side-effects cache keys on: the lockfile-pinned runtime Node
    /// major when one exists, the detected host Node otherwise. `None`
    /// (no `node` on `PATH`) falls back to a deterministic major-0
    /// sentinel; scripts cannot run in that environment anyway, which
    /// mirrors pnpm's `process.version` last resort.
    pub engine: Option<&'a str>,
    /// Read for the registry list when deriving the tarball URL of a
    /// `registry:`-shaped lockfile resolution.
    pub config: &'static Config,
}

/// What the provider materialized.
#[derive(Debug, Default)]
pub struct PackageProviderOutput {
    /// For every installed snapshot, the directory whose
    /// `node_modules/<name>` holds the package. Feeds
    /// [`crate::VirtualStoreLayout::set_provider_paths`] so every
    /// slot-path lookup resolves to the provider directory.
    pub paths: HashMap<PackageKey, PathBuf>,
    /// Optional packages the provider could not build. The caller folds
    /// them into the install-time skip set (persisted to
    /// `.modules.yaml.skipped`) so linking treats them exactly like
    /// installability-skipped optionals.
    pub skipped: Vec<PackageKey>,
}

/// Error type of [`materialize_through_package_provider`].
///
/// The `ERR_PNPM_PACKAGE_PROVIDER_*` codes and message texts match the
/// `PnpmError`s thrown by `@pnpm/installing.deps-restorer`'s
/// `materializeThroughPackageProvider`.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum PackageProviderError {
    #[display(
        "The package provider cannot install {dep_path}: git dependencies that need to be built (prepare) are not supported yet"
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_UNSUPPORTED))]
    GitPrepareUnsupported {
        #[error(not(source))]
        dep_path: String,
    },

    #[display("The package provider does not support the resolution of {dep_path} ({kind})")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_UNSUPPORTED))]
    UnsupportedResolution {
        #[error(not(source))]
        dep_path: String,
        kind: &'static str,
    },

    #[display(
        "The package provider needs the patch file of {dep_path}, but only its hash is known"
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_UNSUPPORTED))]
    PatchWithoutFile {
        #[error(not(source))]
        dep_path: String,
    },

    #[display(
        "The package provider cannot install {dep_path}, which depends on a different version of itself"
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_UNSUPPORTED))]
    SelfDependency {
        #[error(not(source))]
        dep_path: String,
    },

    /// A snapshot has no matching `packages:` row, so there is no
    /// resolution to send. A conforming lockfile never produces this.
    #[display(
        "The package provider cannot install {dep_path}: no matching entry in the lockfile packages section"
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_UNSUPPORTED))]
    MissingPackageMetadata {
        #[error(not(source))]
        dep_path: String,
    },

    #[display("Cannot run the package provider at \"{provider}\": {source}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_FAILED))]
    Spawn {
        provider: String,
        #[error(source)]
        source: io::Error,
    },

    #[display("The package provider at \"{provider}\" exited with code {code}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_FAILED))]
    NonZeroExit {
        #[error(not(source))]
        provider: String,
        /// The numeric exit code, or `unknown` when the provider was
        /// killed by a signal.
        code: String,
    },

    #[display("The package provider at \"{provider}\" did not return valid JSON")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_RESULT_INVALID))]
    InvalidJson {
        #[error(not(source))]
        provider: String,
    },

    #[display(
        "The package provider at \"{provider}\" returned an unsupported response (protocol {protocol})"
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_RESULT_INVALID))]
    UnsupportedResponse {
        #[error(not(source))]
        provider: String,
        /// The reported protocol number, or `missing` when absent.
        protocol: String,
    },

    #[display("The package provider skipped {dep_path}, which is not an optional dependency")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_RESULT_INVALID))]
    SkippedNonOptional {
        #[error(not(source))]
        dep_path: String,
    },

    #[display("The package provider returned no path for {dep_path}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_RESULT_INVALID))]
    MissingPath {
        #[error(not(source))]
        dep_path: String,
    },

    #[display("The package provider returned a relative path for {dep_path}: {dir}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_PROVIDER_RESULT_INVALID))]
    RelativePath {
        #[error(not(source))]
        dep_path: String,
        dir: String,
    },

    #[display("Failed to read the patch file of {dep_path} at {}: {source}", path.display())]
    #[diagnostic(code(pacquet_package_manager::package_provider_read_patch_file))]
    ReadPatchFile {
        dep_path: String,
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to serialize the package provider request: {_0}")]
    #[diagnostic(code(pacquet_package_manager::package_provider_serialize_request))]
    SerializeRequest(#[error(source)] serde_json::Error),
}

/// One node of the protocol-v1 request. Field names are wire contract.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderRequestNode {
    pub(crate) name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) version: Option<String>,
    /// Registry tarball resolution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tarball: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) integrity: Option<String>,
    /// Local directory resolution (`file:` deps and injected workspace
    /// packages) — an install-time snapshot, sent as an absolute path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) directory: Option<String>,
    /// Git resolution, deterministic by commit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) git: Option<ProviderGitSource>,
    pub(crate) deps: BTreeMap<String, ProviderRequestDep>,
    pub(crate) engine: String,
    /// Emitted only when `true`: the provider may skip an optional
    /// package whose build fails instead of aborting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) optional: Option<bool>,
    /// Patches are deterministic, so their content is just another
    /// provider input.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) patch: Option<ProviderPatch>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProviderGitSource {
    pub(crate) repo: String,
    pub(crate) commit: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderRequestDep {
    pub(crate) dep_path: String,
    pub(crate) name: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProviderPatch {
    pub(crate) content: String,
    pub(crate) hash: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderRequest {
    pub(crate) protocol: u32,
    pub(crate) gc_root_dir: String,
    pub(crate) nodes: BTreeMap<String, ProviderRequestNode>,
}

/// The request plus the parsed [`PackageKey`] behind each depPath
/// string, so the response validation can hand typed keys back to the
/// install without re-parsing provider-controlled strings.
#[derive(Debug)]
pub(crate) struct ProviderRequestBundle {
    pub(crate) request: ProviderRequest,
    pub(crate) key_by_dep_path: HashMap<String, PackageKey>,
}

#[derive(Debug, Deserialize)]
struct ProviderResponse {
    protocol: Option<u32>,
    paths: Option<HashMap<String, String>>,
    skipped: Option<Vec<String>>,
}

/// Send the dependency graph to the configured package provider and
/// return the directory it materialized each snapshot at, plus the
/// optional snapshots it skipped. Returns an empty output without
/// spawning the provider when there is nothing to materialize.
pub async fn materialize_through_package_provider(
    inputs: &PackageProviderInputs<'_>,
) -> Result<PackageProviderOutput, PackageProviderError> {
    let Some(bundle) = build_provider_request(inputs)? else {
        return Ok(PackageProviderOutput::default());
    };
    let request_json =
        serde_json::to_string(&bundle.request).map_err(PackageProviderError::SerializeRequest)?;
    let provider = inputs.package_provider.to_string();
    // The provider may run for a long time (e.g. `nix-build`), so the
    // blocking spawn/read runs on the blocking pool instead of stalling
    // the reactor thread.
    let stdout =
        tokio::task::spawn_blocking(move || invoke_provider(&provider, request_json.into_bytes()))
            .await
            .expect("package provider invocation must not panic")?;
    let response = parse_provider_response(inputs.package_provider, &stdout)?;
    validate_provider_response(&bundle, response)
}

/// Build the protocol-v1 request from the lockfile-shaped dependency
/// graph. `Ok(None)` when the graph has no materializable node (the
/// provider is not spawned at all).
pub(crate) fn build_provider_request(
    inputs: &PackageProviderInputs<'_>,
) -> Result<Option<ProviderRequestBundle>, PackageProviderError> {
    let (Some(snapshots), Some(packages)) = (inputs.snapshots, inputs.packages) else {
        return Ok(None);
    };
    // pnpm falls back to its own `process.version` when no runtime pin
    // exists and no `node` is on `PATH`; pacquet has no embedded Node,
    // so a fixed major-0 sentinel keeps the value deterministic (see
    // `PackageProviderInputs::engine`).
    let engine = inputs
        .engine
        .map_or_else(|| pacquet_graph_hasher::engine_name(0, None, None), str::to_string);

    let mut nodes: BTreeMap<String, ProviderRequestNode> = BTreeMap::new();
    let mut key_by_dep_path: HashMap<String, PackageKey> = HashMap::new();
    for (key, snapshot) in snapshots {
        if inputs.skipped.contains(key) {
            continue;
        }
        let dep_path = key.to_string();
        let metadata_key = key.without_peer();
        let Some(metadata) = packages.get(&metadata_key) else {
            return Err(PackageProviderError::MissingPackageMetadata { dep_path });
        };

        let name = key.name.to_string();
        // Same derivation as pnpm's `nameVerFromPkgSnapshot`:
        // `pkgSnapshot.version ?? <semver from the depPath>`, absent
        // for non-semver depPaths (`file:`, git URLs, ...).
        let version = metadata.version.clone().or_else(|| match key.suffix.version() {
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
            engine: engine.clone(),
            optional: snapshot.optional.then_some(true),
            patch: None,
        };
        match &metadata.resolution {
            LockfileResolution::Tarball(_) | LockfileResolution::Registry(_) => {
                // The only reachable error is a tarball resolution
                // without an integrity — registry resolutions always
                // carry one, and the URL derivation is infallible.
                let unsupported = || PackageProviderError::UnsupportedResolution {
                    dep_path: dep_path.clone(),
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
                    Some(pacquet_fs::lexical_normalize(&directory).to_string_lossy().into_owned());
            }
            LockfileResolution::Git(git_resolution) => {
                if metadata.prepare == Some(true) {
                    return Err(PackageProviderError::GitPrepareUnsupported { dep_path });
                }
                node.git = Some(ProviderGitSource {
                    repo: git_resolution.repo.clone(),
                    commit: git_resolution.commit.clone(),
                });
            }
            LockfileResolution::Binary(_) => {
                return Err(PackageProviderError::UnsupportedResolution {
                    dep_path,
                    kind: "binary",
                });
            }
            LockfileResolution::Variations(_) => {
                return Err(PackageProviderError::UnsupportedResolution {
                    dep_path,
                    kind: "variations",
                });
            }
            LockfileResolution::Custom(_) => {
                return Err(PackageProviderError::UnsupportedResolution {
                    dep_path,
                    kind: "custom",
                });
            }
        }

        if let Some(patch_info) = inputs.patches.and_then(|patches| patches.get(&metadata_key)) {
            let Some(patch_file_path) = &patch_info.patch_file_path else {
                return Err(PackageProviderError::PatchWithoutFile { dep_path });
            };
            let content = std::fs::read_to_string(patch_file_path).map_err(|source| {
                PackageProviderError::ReadPatchFile {
                    dep_path: dep_path.clone(),
                    path: patch_file_path.clone(),
                    source,
                }
            })?;
            node.patch = Some(ProviderPatch { content, hash: patch_info.hash.clone() });
        }

        for dep_map in [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()] {
            let Some(dep_map) = dep_map else { continue };
            for (alias, dep_ref) in dep_map {
                // `link:` deps live outside the provider graph.
                let Some(resolved) = dep_ref.resolve(alias) else { continue };
                // Drop edges to nodes not installed (e.g. platform-
                // skipped optionals) so the request stays a closed
                // graph over its own keys.
                if inputs.skipped.contains(&resolved) || !snapshots.contains_key(&resolved) {
                    continue;
                }
                let alias = alias.to_string();
                if alias == name {
                    return Err(PackageProviderError::SelfDependency { dep_path });
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

        key_by_dep_path.insert(dep_path.clone(), key.clone());
        nodes.insert(dep_path, node);
    }

    if nodes.is_empty() {
        return Ok(None);
    }

    let gc_root_dir =
        inputs.lockfile_dir.join("node_modules").join(".pnpm-nix").to_string_lossy().into_owned();
    Ok(Some(ProviderRequestBundle {
        request: ProviderRequest { protocol: PROTOCOL_VERSION, gc_root_dir, nodes },
        key_by_dep_path,
    }))
}

/// Spawn the provider, write the request to its stdin, and return its
/// stdout. Stderr is inherited so provider/Nix build output reaches the
/// user.
fn invoke_provider(provider: &str, request_json: Vec<u8>) -> Result<Vec<u8>, PackageProviderError> {
    let mut child = Command::new(provider)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|source| PackageProviderError::Spawn { provider: provider.to_string(), source })?;
    let mut stdin = child.stdin.take().expect("stdin was piped");
    // Feed stdin from its own thread so a provider that streams output
    // before draining its input cannot deadlock both pipes. A write
    // failure (the provider exited early) is deliberately ignored — the
    // exit status below is the authoritative verdict.
    let writer = std::thread::spawn(move || {
        use std::io::Write as _;
        let _ = stdin.write_all(&request_json);
    });
    let output = child
        .wait_with_output()
        .map_err(|source| PackageProviderError::Spawn { provider: provider.to_string(), source })?;
    writer.join().expect("provider stdin writer must not panic");
    if !output.status.success() {
        return Err(PackageProviderError::NonZeroExit {
            provider: provider.to_string(),
            code: output
                .status
                .code()
                .map_or_else(|| "unknown".to_string(), |code| code.to_string()),
        });
    }
    Ok(output.stdout)
}

fn parse_provider_response(
    provider: &str,
    stdout: &[u8],
) -> Result<ProviderResponse, PackageProviderError> {
    let response: ProviderResponse = serde_json::from_slice(stdout)
        .map_err(|_| PackageProviderError::InvalidJson { provider: provider.to_string() })?;
    if response.protocol != Some(PROTOCOL_VERSION) || response.paths.is_none() {
        return Err(PackageProviderError::UnsupportedResponse {
            provider: provider.to_string(),
            protocol: response
                .protocol
                .map_or_else(|| "missing".to_string(), |protocol| protocol.to_string()),
        });
    }
    Ok(response)
}

/// Enforce the response contract: every `skipped` entry names an
/// optional request node, and every non-skipped request node has a
/// path.
fn validate_provider_response(
    bundle: &ProviderRequestBundle,
    response: ProviderResponse,
) -> Result<PackageProviderOutput, PackageProviderError> {
    let paths = response.paths.expect("checked by parse_provider_response");
    let mut skipped_keys = Vec::new();
    let mut skipped_dep_paths: HashSet<String> = HashSet::new();
    for dep_path in response.skipped.unwrap_or_default() {
        let is_optional =
            bundle.request.nodes.get(&dep_path).is_some_and(|node| node.optional == Some(true));
        if !is_optional {
            return Err(PackageProviderError::SkippedNonOptional { dep_path });
        }
        skipped_keys.push(bundle.key_by_dep_path[&dep_path].clone());
        skipped_dep_paths.insert(dep_path);
    }
    let mut out_paths = HashMap::with_capacity(bundle.key_by_dep_path.len());
    for (dep_path, key) in &bundle.key_by_dep_path {
        if skipped_dep_paths.contains(dep_path) {
            continue;
        }
        let Some(dir) = paths.get(dep_path) else {
            return Err(PackageProviderError::MissingPath { dep_path: dep_path.clone() });
        };
        // A relative or empty path would resolve against the pnpm
        // process directory; reject it like the TypeScript client does.
        if dir.is_empty() || !Path::new(dir).is_absolute() {
            return Err(PackageProviderError::RelativePath {
                dep_path: dep_path.clone(),
                dir: dir.clone(),
            });
        }
        out_paths.insert(key.clone(), PathBuf::from(dir));
    }
    Ok(PackageProviderOutput { paths: out_paths, skipped: skipped_keys })
}

#[cfg(test)]
mod tests;
