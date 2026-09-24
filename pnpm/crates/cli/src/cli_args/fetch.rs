use crate::{State, cli_args::pre_command::fetch_locked_package_manager};
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
    pub(crate) ignore_pnpmfile: bool,
}

impl FetchArgs {
    pub async fn run<Reporter: self::Reporter + 'static>(self, state: State) -> miette::Result<()> {
        fetch_locked_package_manager(state.config, state.lockfile_dir()).await?;
        let lockfile_path = state.lockfile_path();
        let mut fetch_config = (*state.config).clone();
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

        let has_both = self.prod == self.dev;
        let include_prod = has_both || self.prod;
        let include_dev = has_both || self.dev;

        let mut base_install = Install::new(
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
        );
        base_install.lockfile_policy.frozen = true;
        base_install.lockfile_policy.ignore_manifest_check = true;
        base_install.execution.mutation = ProjectMutation::NoInstall;
        base_install.context.lockfile_path = Some(&lockfile_path);
        base_install.run::<Reporter>().await.wrap_err("fetching dependencies")
    }
}
