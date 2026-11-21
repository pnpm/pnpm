//! The install settings the pnpr path carries from the CLI to the server
//! request and the local link pass.

use pnpm_config::NodeLinker;
use pnpm_package_manifest::DependencyGroup;

/// Per-invocation install knobs forwarded to the frozen link pass,
/// already resolved from the CLI flags + config by
/// [`InstallArgs::run`](super::InstallArgs::run).
pub(crate) struct PnprLink<'a> {
    pub(crate) dependency_groups: Vec<DependencyGroup>,
    pub(crate) supported_architectures: Option<pnpm_package_is_installable::SupportedArchitectures>,
    pub(crate) node_linker: NodeLinker,
    pub(crate) skip_runtimes: bool,
    pub(crate) lockfile_path: Option<&'a std::path::Path>,
    pub(crate) use_state_lockfile: bool,
    pub(crate) save_workspace_manifest: bool,
    pub(crate) lockfile: PnprLockfilePolicy,
}

pub(crate) struct PnprLockfilePolicy {
    /// Governs the *server's* resolution behavior (frozen vs
    /// reuse-and-update); forwarded to `/-/pnpr/v0/resolve`. The local
    /// materialization always runs frozen against the server-produced
    /// lockfile.
    pub(crate) frozen: bool,
    /// The *effective* `preferFrozenLockfile` (the CLI tri-state already
    /// resolved against `config.prefer_frozen_lockfile`, exactly as the
    /// local `Install` resolves it); forwarded to `/-/pnpr/v0/resolve`. `false`
    /// forces the server to re-resolve. Resolving the effective value here
    /// keeps a yaml `preferFrozenLockfile:
    /// false` honored on the pnpr path without `--no-prefer-frozen-lockfile`.
    pub(crate) prefer_frozen: bool,
    /// Refresh registry artifacts while retaining every locked package
    /// version. This disables the exchange-free satisfied-lockfile path and
    /// is forwarded to `/-/pnpr/v0/resolve`.
    pub(crate) update_patches: bool,
    /// Regenerate derived lockfile metadata while retaining compatible pins.
    pub(crate) fix: bool,
    /// `--lockfile-only`. Forwarded to `/-/pnpr/v0/resolve` so the server
    /// resolves only and returns the lockfile without fetching files,
    /// after which [`install_via_pnpr`](super::install_via_pnpr) writes the lockfile and skips
    /// materialization, mirroring pnpm's resolve + write, fetch nothing,
    /// link nothing. See
    /// [pnpm/pnpm#12146](https://github.com/pnpm/pnpm/issues/12146).
    pub(crate) only: bool,
    /// `--ignore-manifest-check`; forwarded so the server's frozen
    /// freshness check and the local materialization both skip the
    /// manifest ↔ lockfile comparison.
    pub(crate) ignore_manifest_check: bool,
    /// The effective `trustLockfile` (yaml `trustLockfile` OR
    /// `--trust-lockfile`); forwarded so the server skips verifying the
    /// input lockfile when the user opted out, mirroring the local path.
    pub(crate) trust: bool,
}
