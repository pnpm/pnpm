use crate::State;
use clap::Args;
use miette::Context;
use pnpm_package_manager::{Install, ProjectMutation};
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::Reporter;

#[derive(Debug, Args)]
pub struct FetchArgs {
    #[clap(short = 'P', long, visible_alias = "production")]
    prod: bool,
    #[clap(short = 'D', long)]
    dev: bool,

    /// Disable pnpm hooks defined in `.pnpmfile.cjs`, including the
    /// pnpmfiles of config dependencies.
    #[clap(long = "ignore-pnpmfile")]
    ignore_pnpmfile: bool,
}

impl FetchArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(self, state: State) -> miette::Result<()> {
        let lockfile_path = state.lockfile_path();
        let mut fetch_config = (*state.config).clone();
        fetch_config.ignore_pnpmfile = self.ignore_pnpmfile || fetch_config.ignore_pnpmfile;
        fetch_config.virtual_store_only = true;
        fetch_config.enable_modules_dir = true;
        fetch_config.apply_virtual_store_only_derivation();
        let fetch_config = fetch_config.leak();
        let State {
            tarball_mem_cache,
            http_client,
            config: _,
            manifest,
            lockfile,
            resolved_packages,
        } = &state;

        // `ignore_pnpmfile` is already folded into `fetch_config` above.
        let &FetchArgs { prod, dev, ignore_pnpmfile: _ } = &self;
        let has_both = prod == dev;
        let include_prod = has_both || prod;
        let include_dev = has_both || dev;

        Install {
            tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
            http_client,
            http_client_arc: std::sync::Arc::clone(http_client),
            config: fetch_config,
            manifest,
            emit_initial_manifest: true,
            lockfile: pnpm_lockfile::MaybeLazyLockfile::Lazy(lockfile),
            lockfile_path: Some(&lockfile_path),
            // Optional dependencies follow production, so `--dev` (which
            // excludes production) excludes optional deps too.
            dependency_groups: std::iter::empty()
                .chain(include_prod.then_some(DependencyGroup::Prod))
                .chain(include_dev.then_some(DependencyGroup::Dev))
                .chain(include_prod.then_some(DependencyGroup::Optional)),
            frozen_lockfile: true,
            prefer_frozen_lockfile: None,
            ignore_manifest_check: true,
            // Honor the yaml/npmrc `skipRuntimes` / `trustLockfile`. Fetch
            // exposes no CLI override for either, so the config value is
            // the resolved value, mirroring `pacquet install`.
            skip_runtimes: fetch_config.skip_runtimes,
            trust_lockfile: fetch_config.trust_lockfile,
            update_checksums: false,
            mutation: ProjectMutation::NoInstall,
            installs_only: true,
            resolved_packages,
            supported_architectures: fetch_config.supported_architectures.clone(),
            node_linker: fetch_config.node_linker,
            lockfile_only: false,
            dry_run: false,
            persist_policy_excludes: false,
            update_seed_policy: pnpm_package_manager::UpdateSeedPolicy::KeepAll,
            preferred_versions_override: None,
            auth_override: None,
            resolution_observer: None,
            peer_issues_sink: None,
            deps_requiring_build_sink: None,
            catalogs_override: None,
            disable_optimistic_repeat_install: false,
            pnpmfile_hook_override: None,
            workspace_projects_override: None,
        }
        .run::<Reporter>()
        .await
        .wrap_err("fetching dependencies")
    }
}
