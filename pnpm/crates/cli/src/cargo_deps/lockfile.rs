use super::{
    Arc, BTreeMap, BTreeSet, CargoLockfilePolicy, CargoResolveOptions, CargoWorkspaceMetadata,
    Command, Config, FromStr, GitPackage, GitSource, IntoDiagnostic, Path, PathBuf, PnprClient,
    Result, StreamExt, ThrottledClient, TryStreamExt, WORKSPACE_INSTALL_CONCURRENCY,
    fetch_sparse_index, fs, is_crates_io, stream,
};
use miette::WrapErr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LockedCrate {
    pub(super) name: String,
    pub(super) version: String,
    pub(super) checksum: String,
}

/// What a `Cargo.lock` asks the install to provide, grouped by the kind of
/// source each package comes from. Workspace members carry no source and
/// are already on disk, so they appear in neither list.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct LockedPackages {
    pub(super) crates: Vec<LockedCrate>,
    pub(super) git: Vec<GitPackage>,
}

impl LockedPackages {
    /// The git sources the locked packages come from, deduplicated so the
    /// managed Cargo configuration declares each one once.
    pub(super) fn git_sources(&self) -> Vec<GitSource> {
        let sources: BTreeSet<_> =
            self.git.iter().map(|package| GitSource::clone(&package.source)).collect();
        sources.into_iter().collect()
    }
}

pub(crate) async fn workspace_root(manifest_path: &Path) -> Result<PathBuf> {
    workspace_metadata(manifest_path).await.map(|metadata| metadata.workspace_root)
}

async fn workspace_metadata(manifest_path: &Path) -> Result<CargoWorkspaceMetadata> {
    let metadata = read_cargo_metadata_for_manifest(manifest_path).await?;
    serde_json::from_str::<CargoWorkspaceMetadata>(&metadata)
        .into_diagnostic()
        .wrap_err("read Cargo workspace root from metadata")
}

pub(super) async fn discover_workspace_roots(manifests: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut pending = manifests
        .iter()
        .map(|manifest| canonical_cargo_path(manifest))
        .collect::<Result<BTreeSet<_>>>()?;
    let mut roots = BTreeSet::new();
    while !pending.is_empty() {
        let concurrency = if roots.is_empty() { 1 } else { WORKSPACE_INSTALL_CONCURRENCY };
        let batch =
            std::iter::from_fn(|| pending.pop_first()).take(concurrency).collect::<Vec<_>>();
        let metadata = stream::iter(batch)
            .map(|manifest| async move { workspace_metadata(&manifest).await })
            .buffer_unordered(concurrency)
            .try_collect::<Vec<_>>()
            .await?;
        for workspace in metadata {
            let root = canonical_cargo_path(&workspace.workspace_root)?;
            pending.remove(&root.join("Cargo.toml"));
            for package in workspace.packages {
                pending.remove(&canonical_cargo_path(&package.manifest_path)?);
            }
            roots.insert(root);
        }
    }
    Ok(roots.into_iter().collect())
}

fn canonical_cargo_path(path: &Path) -> Result<PathBuf> {
    dunce::canonicalize(path)
        .into_diagnostic()
        .wrap_err_with(|| format!("resolve Cargo workspace path {}", path.display()))
}

pub(super) async fn read_or_resolve_lockfile(
    config: &Config,
    root_dir: &Path,
    cargo_lock_path: &Path,
    frozen_lockfile: bool,
    lockfile_policy: CargoLockfilePolicy,
    http_client: &Arc<ThrottledClient>,
) -> Result<String> {
    if lockfile_policy == CargoLockfilePolicy::UseExisting {
        match fs::read_to_string(cargo_lock_path) {
            Ok(lockfile) => return Ok(lockfile),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("read {}", cargo_lock_path.display()));
            }
        }
    }
    if frozen_lockfile {
        return Err(miette::miette!(
            "Cargo.lock is absent, but --frozen-lockfile forbids generating it"
        ));
    }
    reject_redirected_workspace(root_dir)?;

    let metadata = read_cargo_metadata(root_dir).await?;
    if let Some(lockfile) = resolve_via_pnpr(config, &metadata).await? {
        return Ok(lockfile);
    }
    let index_files = fetch_sparse_index(config, &metadata, http_client).await?;
    let source = pnpm_cargo_resolver::registry_source(&config.cargo.index_url);
    let lockfile = pnpm_cargo_resolver::resolve_lockfile(&metadata, &index_files, &source)
        .wrap_err("resolve Cargo dependencies")?;
    Ok(lockfile)
}

/// Refuse to resolve a workspace whose root manifest redirects a dependency
/// to another source.
///
/// `[patch]` and `[replace]` change which package a requirement resolves to,
/// and `cargo metadata` does not report either, so a lockfile resolved from
/// it would name the replaced package. An existing `Cargo.lock` is read
/// rather than resolved, which is how a workspace with a patch installs.
fn reject_redirected_workspace(root_dir: &Path) -> Result<()> {
    let manifest_path = root_dir.join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", manifest_path.display()))?;
    let document: toml::Table = toml::from_str(&manifest)
        .into_diagnostic()
        .wrap_err_with(|| format!("parse {}", manifest_path.display()))?;
    for table in ["patch", "replace"] {
        if document.contains_key(table) {
            let manifest_path = manifest_path.display();
            return Err(miette::miette!(
                "{manifest_path} declares [{table}], which pnpm cannot resolve. Commit the Cargo.lock that `cargo generate-lockfile` writes for it.",
            ));
        }
    }
    Ok(())
}

/// Resolve through the configured pnpr server, which walks the sparse
/// index server-side instead of making this client fetch one index file
/// per crate in the graph.
///
/// `None` when there is no server to ask, or when the one configured
/// resolves npm alone: a server that does not serve Cargo resolution
/// means a local resolve, not a failed install.
pub(super) async fn resolve_via_pnpr(config: &Config, metadata: &str) -> Result<Option<String>> {
    let Some(pnpr_server) = config.pnpr_server.as_deref().filter(|_| !config.offline) else {
        return Ok(None);
    };
    let client = PnprClient::new(pnpr_server);
    if !pnpm_pnpr_client::server_resolves(&client, pnpr_server, pnpm_pnpr_client::CARGO_ECOSYSTEM)
        .await?
    {
        return Ok(None);
    }
    // Only the dependency graph leaves the machine: the rest of a
    // `cargo metadata` document describes local paths the server has no
    // use for.
    let metadata = pnpm_cargo_resolver::resolve_inputs(metadata)
        .wrap_err("reduce cargo metadata for the pnpr server")?;
    client
        .resolve_cargo(CargoResolveOptions {
            metadata,
            registry: config.cargo.index_url.clone(),
            authorization: config.auth_headers.for_url(pnpr_server),
        })
        .await
        .into_diagnostic()
        .wrap_err("resolve Cargo dependencies through the pnpr server")
        .map(Some)
}

async fn read_cargo_metadata(root_dir: &Path) -> Result<String> {
    read_cargo_metadata_for_manifest(&root_dir.join("Cargo.toml")).await
}

async fn read_cargo_metadata_for_manifest(manifest_path: &Path) -> Result<String> {
    let manifest_path = manifest_path.to_path_buf();
    let output = tokio::task::spawn_blocking(move || {
        Command::new("cargo")
            .args(["metadata", "--no-deps", "--format-version", "1", "--manifest-path"])
            .arg(&manifest_path)
            .output()
            .map(|output| (manifest_path, output))
    })
    .await
    .into_diagnostic()
    .wrap_err("join Cargo manifest discovery task")?
    .into_diagnostic()
    .wrap_err("run cargo metadata for manifest discovery")?;
    let (manifest_path, output) = output;
    if !output.status.success() {
        let manifest_path = manifest_path.display();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        return Err(miette::miette!("cargo metadata failed for {}: {}", manifest_path, stderr,));
    }
    String::from_utf8(output.stdout).into_diagnostic().wrap_err("decode cargo metadata output")
}

pub(super) fn parse_lockfile(input: &str, index_url: &str) -> Result<LockedPackages> {
    let lockfile =
        cargo_lock::Lockfile::from_str(input).into_diagnostic().wrap_err("parse Cargo.lock")?;
    let mut packages = LockedPackages::default();
    // One repository serves every package a checkout of it provides, so the
    // packages of a source share the description of it.
    let mut git_sources: BTreeMap<String, Arc<GitSource>> = BTreeMap::new();
    for package in lockfile.packages {
        let Some(source) = package.source.as_ref() else {
            continue;
        };
        if source.is_git() {
            let name = package.name.to_string();
            let version = package.version.to_string();
            validate_package_identity(&name, &version)?;
            let source = match git_sources.entry(source.to_string()) {
                std::collections::btree_map::Entry::Occupied(known) => Arc::clone(known.get()),
                std::collections::btree_map::Entry::Vacant(slot) => {
                    Arc::clone(slot.insert(Arc::new(GitSource::from_source_id(source)?)))
                }
            };
            packages.git.push(GitPackage { name, version, source });
            continue;
        }
        let crate_source = source.clone();
        packages.crates.push(locked_crate_from_package(package, &crate_source, index_url)?);
    }

    reject_duplicate_links("registry", packages.crates.iter().map(LockedCrate::link_name))?;
    reject_duplicate_links("git", packages.git.iter().map(GitPackage::link_name))?;
    Ok(packages)
}

/// Two packages that link into one Cargo directory source under the same
/// name are the same package to `cargo`, whichever of them it reads.
fn reject_duplicate_links(kind: &str, link_names: impl Iterator<Item = String>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for link_name in link_names {
        if !seen.insert(link_name.clone()) {
            return Err(miette::miette!(
                "Cargo.lock contains duplicate {kind} package {link_name}",
            ));
        }
    }
    Ok(())
}

fn locked_crate_from_package(
    package: cargo_lock::Package,
    source: &cargo_lock::SourceId,
    index_url: &str,
) -> Result<LockedCrate> {
    // crates.io is spelled two ways in a lockfile — the canonical git
    // identifier and the sparse index — and `cargo` writes either.
    let expected_source = pnpm_cargo_resolver::registry_source(index_url);
    let matches_registry = if is_crates_io(index_url) {
        source.is_default_registry()
    } else {
        source.to_string() == expected_source
    };
    if !matches_registry {
        return Err(miette::miette!(
            "Cargo source {source:?} does not match the configured Cargo registry {expected_source:?}"
        ));
    }
    let name = package.name.to_string();
    let version = package.version.to_string();
    let checksum = package
        .checksum
        .ok_or_else(|| miette::miette!("registry package {name} {version} has no checksum"))?
        .to_string();
    validate_package_identity(&name, &version)?;
    if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(miette::miette!("invalid checksum for crate {name} {version}"));
    }
    Ok(LockedCrate { name, version, checksum: checksum.to_ascii_lowercase() })
}

/// Both names reach the filesystem as the `<name>-<version>` directory a
/// Cargo source is linked under.
fn validate_package_identity(name: &str, version: &str) -> Result<()> {
    validate_package_field("crate name", name, |byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
    })?;
    validate_package_field("crate version", version, |byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+')
    })
}

pub(super) fn validate_package_field(
    label: &str,
    value: &str,
    allowed: impl Fn(u8) -> bool,
) -> Result<()> {
    if value.is_empty() || !value.bytes().all(allowed) {
        return Err(miette::miette!("invalid {label} {value:?} in Cargo.lock"));
    }
    Ok(())
}

impl LockedCrate {
    pub(super) fn link_name(&self) -> String {
        format!("{}-{}", self.name, self.version)
    }

    /// The shared slot contains immutable crate source, not a dependency view.
    /// Unlike an npm GVS slot, it has no package-local dependency links, so its
    /// final identity component is the registry checksum rather than a graph
    /// hash. Cargo's workspace directory source supplies the graph-specific
    /// view, and Cargo writes compilation artifacts outside this slot.
    pub(super) fn store_slot(&self, store_root: &Path) -> PathBuf {
        store_root.join("crates").join(&self.name).join(&self.version).join(&self.checksum)
    }
}
