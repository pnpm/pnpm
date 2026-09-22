use super::{
    LockfileDirArg,
    NodeLinkerArg,
};
#[derive(Debug, Default, Clone, clap::Args)]
pub struct InstallLockfileArgs {
    /// Don't generate a lockfile, and fail if an update to it is needed. This
    /// setting is enabled by default in CI when a lockfile is present.
    #[clap(long = "frozen-lockfile", overrides_with = "no_frozen_lockfile")]
    #[clap(id = "frozen_lockfile")]
    pub frozen: bool,
    /// Allow the lockfile to be updated, overriding a `frozenLockfile: true`
    /// setting.
    #[clap(long = "no-frozen-lockfile", overrides_with = "frozen_lockfile")]
    #[clap(id = "no_frozen_lockfile")]
    pub no_frozen: bool,
    /// Only update `pnpm-lock.yaml`. Don't download packages or write
    /// `node_modules`.
    #[clap(long = "lockfile-only")]
    #[clap(id = "lockfile_only")]
    pub only: bool,
    /// Repair broken lockfile entries by re-resolving their metadata while
    /// preserving compatible locked versions.
    #[clap(long = "fix-lockfile")]
    #[clap(id = "fix_lockfile")]
    pub fix: bool,
    #[clap(flatten)]
    pub directory: LockfileDirArg,
    /// Prefer the existing lockfile over re-resolving, even when the
    /// manifest may have changed.
    #[clap(long = "prefer-frozen-lockfile", overrides_with = "no_prefer_frozen_lockfile")]
    #[clap(id = "prefer_frozen_lockfile")]
    pub prefer_frozen: bool,
    /// Always re-resolve against the registry instead of preferring the
    /// existing lockfile.
    #[clap(long = "no-prefer-frozen-lockfile", overrides_with = "prefer_frozen_lockfile")]
    #[clap(id = "no_prefer_frozen_lockfile")]
    pub no_prefer_frozen: bool,
    /// Skip the check that `pnpm-lock.yaml` is up to date with
    /// `package.json` under `--frozen-lockfile`. For callers that just
    /// wrote the lockfile themselves and know the manifest is about to
    /// catch up.
    #[clap(long)]
    pub ignore_manifest_check: bool,
}

#[derive(Debug, Default, Clone, clap::Args)]
pub struct LockfileUpdateArgs {
    #[clap(flatten)]
    pub dedupe: crate::cli_args::install_options::AutoDedupeArgs,
    /// Fold every per-branch lockfile (`pnpm-lock.<branch>.yaml`, written
    /// under the `gitBranchLockfile` setting) into `pnpm-lock.yaml` and
    /// delete them.
    #[clap(long = "merge-git-branch-lockfiles")]
    pub merge_git_branch_lockfiles: bool,
    /// Glob patterns naming the branches that merge the per-branch
    /// lockfiles, so a mainline branch does not have to pass
    /// `--merge-git-branch-lockfiles` by hand.
    #[clap(long = "merge-git-branch-lockfiles-branch-pattern")]
    pub merge_git_branch_lockfiles_branch_pattern: Vec<String>,
    /// Skip verifying the lockfile against supply-chain policies.
    #[clap(long = "trust-lockfile", overrides_with = "no_trust_lockfile")]
    pub trust_lockfile: bool,
    /// Verify the lockfile against supply-chain policies even when the
    /// configuration trusts it.
    #[clap(long = "no-trust-lockfile", overrides_with = "trust_lockfile")]
    pub no_trust_lockfile: bool,
    /// Refresh the integrity checksums in `pnpm-lock.yaml` from the
    /// registry. Cannot be combined with `--frozen-lockfile`.
    #[clap(long = "update-checksums")]
    pub update_checksums: bool,
}

#[derive(Debug, Default, Clone, clap::Args)]
pub struct InstallFetchArgs {
    /// Maximum number of concurrent network requests during install.
    #[clap(long = "network-concurrency")]
    #[clap(id = "network_concurrency")]
    pub concurrency: Option<usize>,
    /// Per-request network timeout, in milliseconds.
    #[clap(long = "fetch-timeout")]
    #[clap(id = "fetch_timeout")]
    pub timeout: Option<u64>,
    /// Warn when a registry metadata request takes longer than this many
    /// milliseconds.
    #[clap(long = "fetch-warn-timeout-ms")]
    #[clap(id = "fetch_warn_timeout_ms")]
    pub warn_timeout_ms: Option<u64>,
    /// Warn when a tarball download's average speed is below this many KiB/s.
    #[clap(long = "fetch-min-speed-ki-bps")]
    #[clap(id = "fetch_min_speed_ki_bps")]
    pub min_speed_ki_bps: Option<u64>,
    /// `User-Agent` header to send on registry requests.
    #[clap(long = "user-agent")]
    pub user_agent: Option<String>,
    /// URL of a pnpr server to offload resolution and file fetching to.
    /// `node_modules` is still linked locally from the server-produced
    /// lockfile.
    #[clap(long = "pnpr-server")]
    pub pnpr_server: Option<String>,
}

#[derive(Debug, Default, Clone, clap::Args)]
pub struct InstallMaterializationArgs {
    /// Show what an install would change without writing anything to disk.
    #[clap(long = "dry-run")]
    pub dry_run: bool,
    /// Reinstall every package the lockfile names: relink packages an
    /// earlier install already materialized, and install optional
    /// dependencies whose `cpu` / `os` / `libc` / `engines` don't match
    /// the host instead of skipping them.
    #[clap(long)]
    pub force: bool,
    /// Run the install already requested by `verifyDepsBeforeRun` without
    /// independently short-circuiting it as up to date.
    #[clap(long, hide = true)]
    pub verify_deps_before_run_install: bool,
    /// Don't install runtime dependencies (`node`, `deno`, `bun`). Their
    /// archives aren't fetched and their bins aren't linked; the rest of
    /// the install proceeds normally.
    #[clap(long = "no-runtime")]
    pub no_runtime: bool,
    /// Which node linker to use: `isolated` (the default, a symlinked
    /// store), `hoisted` (a flat `node_modules`), or `pnp` (Plug'n'Play).
    /// Overrides the configured value.
    #[clap(long = "node-linker", value_enum)]
    pub node_linker: Option<NodeLinkerArg>,
    /// Open the store read-only and skip all store writes. For installing
    /// against a store on a read-only filesystem (e.g. a Nix store); pair
    /// with `--offline --frozen-lockfile`.
    #[clap(long = "frozen-store", overrides_with = "no_frozen_store")]
    pub frozen_store: bool,
    /// Allow store writes even when the configuration enables the
    /// read-only store.
    #[clap(long = "no-frozen-store", overrides_with = "frozen_store")]
    pub no_frozen_store: bool,
}
