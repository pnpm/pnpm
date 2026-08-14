//! Resolve a package manager's version specifier and materialize it,
//! returning what a caller needs to run it.

use miette::{Context, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_reporter::Reporter;
use std::path::{Path, PathBuf};

use crate::{
    config_deps,
    engine_pm::{
        channel::{BinaryChannel, Channel, EnginePackages, PackageManager},
        error::EngineError,
        install::{engine_env_root, install_engine_to_store},
    },
    shim_dispatch::materialize_runtime,
};

/// The Node.js line a JavaScript package manager runs on when the host has
/// no `node` of its own. LTS is the conservative pick, and — being a stable
/// release — it is the one channel whose archives carry Node's own
/// publisher signatures.
const MANAGED_NODE_SPEC: &str = "lts";

/// A package manager, installed and ready to run.
pub(crate) struct ProvisionedEngine {
    /// The executable to spawn.
    pub(crate) program: PathBuf,
    /// Directories to prepend to `PATH` before spawning it, most
    /// significant first.
    pub(crate) bin_dirs: Vec<PathBuf>,
}

/// Install `pm` at `version_spec` and return how to run it.
///
/// The bytes are resolved and fetched through the trusted package-manager
/// bootstrap configuration, never through repository-controlled project
/// settings, so a cloned repository cannot steer the download.
pub(crate) async fn provision<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    pm: PackageManager,
    version_spec: &str,
) -> miette::Result<ProvisionedEngine> {
    match pm.channel(version_spec) {
        Channel::Registry { package } => {
            provision_from_registry::<Reporter>(config, pm, package, version_spec).await
        }
        Channel::Binary(BinaryChannel::Bun) => provision_bun(version_spec).await,
        Channel::Binary(BinaryChannel::Yarn) => {
            Err(EngineError::UnsupportedChannel { spec: version_spec.to_string() }.into())
        }
    }
}

async fn provision_from_registry<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    pm: PackageManager,
    package: &str,
    version_spec: &str,
) -> miette::Result<ProvisionedEngine> {
    let name = pm.name();
    let resolved = config_deps::resolve_engine_version(config, package, version_spec)
        .await?
        .ok_or_else(|| EngineError::cannot_resolve(pm, version_spec))?;
    let env_root = engine_env_root(config, pm)?;
    let bin_dir = Box::pin(install_engine_to_store::<Reporter>(
        config,
        pm,
        &env_root,
        version_spec,
        &resolved.version,
        false,
    ))
    .await?;

    // Resolve the bin strictly within the engine's own directory, never the
    // full PATH, so a missing shim is an error rather than a silent
    // fall-through to whatever package manager happens to be installed.
    let program = which::which_in(name, Some(&bin_dir), &bin_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("locate {name} in the installed engine"))?;

    let mut bin_dirs = vec![bin_dir];
    let packages = pm
        .engine_packages(&resolved.version)
        .ok_or_else(|| miette::miette!("{name}@{} is not a registry engine", resolved.version))?;
    if let Some(node_bin_dir) = node_bin_dir(packages).await? {
        bin_dirs.push(node_bin_dir);
    }
    Ok(ProvisionedEngine { program, bin_dirs })
}

/// Bun's package manager and its runtime are the same executable, so
/// provisioning it is exactly the managed-runtime install.
async fn provision_bun(version_spec: &str) -> miette::Result<ProvisionedEngine> {
    let program = materialize_runtime("bun".to_string(), version_spec.to_string()).await?;
    let bin_dirs = program.parent().map(Path::to_path_buf).into_iter().collect();
    Ok(ProvisionedEngine { program, bin_dirs })
}

/// The directory holding a `node` for a JavaScript engine to run on, or
/// `None` when the engine needs none — either because it is a native
/// binary, or because the host already has a `node` on `PATH`.
///
/// A machine that only ever installed pnpm has no Node.js at all, and npm
/// and Yarn cannot start without one, so pnpm installs the runtime it
/// already knows how to manage rather than failing.
async fn node_bin_dir(packages: EnginePackages) -> miette::Result<Option<PathBuf>> {
    if packages.links_native_binary || which::which("node").is_ok() {
        return Ok(None);
    }
    let node = materialize_runtime("node".to_string(), MANAGED_NODE_SPEC.to_string())
        .await
        .wrap_err("install a Node.js runtime to run the package manager with")?;
    Ok(node.parent().map(Path::to_path_buf))
}
