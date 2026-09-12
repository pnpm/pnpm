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
            lockfile_path: Some(&lockfile_path),
            frozen_lockfile: true,
            ignore_manifest_check: true,
            mutation: ProjectMutation::NoInstall,
            ..Install::new(
                std::sync::Arc::clone(tarball_mem_cache),
                resolved_packages,
                (http_client, std::sync::Arc::clone(http_client)),
                fetch_config,
                manifest,
                pnpm_lockfile::MaybeLazyLockfile::Lazy(lockfile),
                std::iter::empty()
                    .chain(include_prod.then_some(DependencyGroup::Prod))
                    .chain(include_dev.then_some(DependencyGroup::Dev))
                    .chain(include_prod.then_some(DependencyGroup::Optional)),
            )
        }
        .run::<Reporter>()
        .await
        .wrap_err("fetching dependencies")
    }
}
