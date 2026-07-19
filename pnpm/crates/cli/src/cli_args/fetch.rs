use crate::State;
use clap::Args;
use miette::Context;
use pacquet_package_manager::Install;
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::Reporter;

#[derive(Debug, Args)]
pub struct FetchArgs {
    #[clap(short = 'P', long)]
    prod: bool,
    #[clap(short = 'D', long)]
    dev: bool,
}

impl FetchArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(self, state: State) -> miette::Result<()> {
        let lockfile_path = state.lockfile_path();
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &state;

        let &FetchArgs { prod, dev } = &self;
        let has_both = prod == dev;
        let include_prod = has_both || prod;
        let include_dev = has_both || dev;

        Install {
            tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
            http_client,
            http_client_arc: std::sync::Arc::clone(http_client),
            config,
            manifest,
            emit_initial_manifest: true,
            lockfile: pacquet_lockfile::MaybeLazyLockfile::Lazy(lockfile),
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
            skip_runtimes: config.skip_runtimes,
            trust_lockfile: config.trust_lockfile,
            update_checksums: false,
            is_full_install: false,
            installs_only: true,
            resolved_packages,
            supported_architectures: config.supported_architectures.clone(),
            node_linker: config.node_linker,
            lockfile_only: false,
            dry_run: false,
            update_seed_policy: pacquet_package_manager::UpdateSeedPolicy::KeepAll,
            auth_override: None,
            resolution_observer: None,
            peer_issues_sink: None,
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
