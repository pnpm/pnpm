//! Resolve a package manager's version specifier and materialize it,
//! returning what a caller needs to run it.

use miette::Context;
use pnpm_config::Config;
use pnpm_reporter::Reporter;
use std::path::{Path, PathBuf};

use crate::{
    engine_pm::{
        channel::{BinaryChannel, Channel, EnginePackages, PackageManager},
        error::EngineError,
        install::{engine_env_root, install_engine_to_store},
        resolve::resolve_release,
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

impl ProvisionedEngine {
    /// The executable for the command `name`, falling back to the
    /// engine's main program when it publishes no such command.
    ///
    /// A package manager publishes more than one — `npx` alongside `npm` —
    /// so the one the user typed is what runs. Only the engine's own
    /// directory answers: a managed Node.js behind it ships an `npm` and
    /// an `npx` of its own, which are not the versions that were asked
    /// for.
    pub(crate) fn command(&self, name: &str) -> PathBuf {
        self.bin_dirs
            .first()
            .and_then(|bin_dir| engine_bin(bin_dir, name))
            .unwrap_or_else(|| self.program.clone())
    }
}

/// Locate `name` strictly inside `bin_dir`, never on the wider `PATH`, so
/// a missing command is an error rather than a silent fall-through to
/// whatever package manager the host happens to have installed.
/// `which_in` is used only to pick the platform-correct shim name (e.g.
/// `pnpm.cmd` on Windows).
pub(crate) fn engine_bin(bin_dir: &Path, name: &str) -> Option<PathBuf> {
    which::which_in(name, Some(bin_dir), bin_dir).ok()
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
        Channel::Binary(binary) => provision_binary(config, binary, version_spec).await,
    }
}

/// A package manager that ships as a platform archive is exactly what the
/// managed-runtime installer already materializes: pinned by a publisher
/// checksum, unpacked into the global virtual store, executed from there.
async fn provision_binary(
    config: &Config,
    binary: BinaryChannel,
    version_spec: &str,
) -> miette::Result<ProvisionedEngine> {
    let name = match binary {
        BinaryChannel::Bun => "bun",
        BinaryChannel::Yarn => "yarn",
    };
    let program =
        materialize_runtime(&config.state_dir, name.to_string(), version_spec.to_string()).await?;
    let bin_dir = program.parent().ok_or_else(|| EngineError::MissingEngineBin {
        name,
        dir: program.display().to_string(),
    })?;
    Ok(ProvisionedEngine { bin_dirs: vec![bin_dir.to_path_buf()], program })
}

async fn provision_from_registry<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    pm: PackageManager,
    package: &str,
    version_spec: &str,
) -> miette::Result<ProvisionedEngine> {
    let name = pm.name();
    let resolved = resolve_release(config, pm, package, version_spec).await?;
    let env_root = engine_env_root(config, pm)?;
    let bin_dir = Box::pin(install_engine_to_store::<Reporter>(
        config,
        pm,
        &env_root,
        version_spec,
        &resolved.version,
        // A foreign package manager's closure is recorded in a global env
        // lockfile under the pnpm home directory, never in the project's
        // `pnpm-lock.yaml`, so `--frozen-lockfile` has nothing to freeze here.
        false,
        false,
    ))
    .await?;

    let program = engine_bin(&bin_dir, name).ok_or_else(|| EngineError::MissingEngineBin {
        name,
        dir: bin_dir.display().to_string(),
    })?;

    let mut bin_dirs = vec![bin_dir];
    let packages = pm
        .engine_packages(&resolved.version)
        .ok_or_else(|| miette::miette!("{name}@{} is not a registry engine", resolved.version))?;
    if let Some(node_bin_dir) = node_bin_dir(config, packages).await? {
        bin_dirs.push(node_bin_dir);
    }
    Ok(ProvisionedEngine { program, bin_dirs })
}

/// The directory holding a `node` for a JavaScript engine to run on, or
/// `None` when the engine needs none — either because it is a native
/// binary, or because the host already has a `node` on `PATH`.
///
/// A machine that only ever installed pnpm has no Node.js at all, and npm
/// and Yarn cannot start without one, so pnpm installs the runtime it
/// already knows how to manage rather than failing.
async fn node_bin_dir(
    config: &Config,
    packages: EnginePackages,
) -> miette::Result<Option<PathBuf>> {
    if packages.links_native_binary || which::which("node").is_ok() {
        return Ok(None);
    }
    let node =
        materialize_runtime(&config.state_dir, "node".to_string(), MANAGED_NODE_SPEC.to_string())
            .await
            .wrap_err("install a Node.js runtime to run the package manager with")?;
    Ok(node.parent().map(Path::to_path_buf))
}
