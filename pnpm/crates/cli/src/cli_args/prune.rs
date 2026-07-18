use crate::State;
use clap::Args;
use miette::Context;
use pacquet_package_manager::Install;
use pacquet_package_manifest::DependencyGroup;
use pacquet_reporter::Reporter;

#[derive(Debug, Args)]
pub struct PruneArgs {
    #[clap(short = 'P', long)]
    prod: bool,
    #[clap(short = 'D', long)]
    dev: bool,
    #[clap(long)]
    no_optional: bool,
    #[clap(long = "ignore-scripts", overrides_with = "no_ignore_scripts")]
    pub ignore_scripts: bool,
    /// Run lifecycle scripts even if scripts are disabled by configuration.
    #[clap(long = "no-ignore-scripts", overrides_with = "ignore_scripts")]
    pub no_ignore_scripts: bool,
}

impl PruneArgs {
    fn dependency_groups(&self) -> impl Iterator<Item = DependencyGroup> {
        let &PruneArgs { prod, dev, no_optional, ignore_scripts: _, no_ignore_scripts: _ } = self;
        let has_both = prod == dev;
        let has_prod = has_both || prod;
        let has_dev = has_both || dev;
        let has_optional = !no_optional;
        std::iter::empty()
            .chain(has_prod.then_some(DependencyGroup::Prod))
            .chain(has_dev.then_some(DependencyGroup::Dev))
            .chain(has_optional.then_some(DependencyGroup::Optional))
    }

    pub async fn run<Reporter: self::Reporter + 'static>(self, state: State) -> miette::Result<()> {
        let lockfile_path = state.lockfile_path();
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &state;

        let dependency_groups: Vec<DependencyGroup> = self.dependency_groups().collect();

        Install {
            tarball_mem_cache: std::sync::Arc::clone(tarball_mem_cache),
            http_client,
            http_client_arc: std::sync::Arc::clone(http_client),
            config,
            manifest,
            emit_initial_manifest: true,
            lockfile: pacquet_lockfile::MaybeLazyLockfile::Lazy(lockfile),
            lockfile_path: Some(&lockfile_path),
            dependency_groups,
            frozen_lockfile: false,
            prefer_frozen_lockfile: None,
            ignore_manifest_check: false,
            skip_runtimes: false,
            trust_lockfile: false,
            update_checksums: false,
            is_full_install: true,
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
            disable_optimistic_repeat_install: true,
            pnpmfile_hook_override: None,
            workspace_projects_override: None,
        }
        .run::<Reporter>()
        .await
        .wrap_err("pruning dependencies")
    }
}
