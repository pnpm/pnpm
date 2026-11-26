use crate::{
    State,
    cli_args::{
        install::workspace_install_selection,
        pipelines::InstallFamilySelection,
        pre_command::fetch_locked_package_manager,
    },
};
use clap::Args;
use miette::Context;
use pnpm_package_manager::ProjectMutation;
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::Reporter;

#[derive(Debug, Clone, Args)]
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
    /// `selection` narrows a shared workspace lockfile to the importers a
    /// `--filter` selected; `None` fetches every importer.
    pub(crate) async fn run<Reporter: self::Reporter + 'static>(
        self,
        state: State,
        selection: Option<&InstallFamilySelection>,
    ) -> miette::Result<()> {
        fetch_locked_package_manager::<Reporter>(state.config, state.lockfile_dir()).await?;
        let lockfile_path = state.lockfile_path();
        let mut fetch_config = (*state.config).clone();
        fetch_config.virtual_store_only = true;
        fetch_config.enable_modules_dir = true;
        fetch_config.apply_virtual_store_only_derivation();
        let fetch_config = fetch_config.leak();
        let has_both = self.prod == self.dev;
        let include_prod = has_both || self.prod;
        let include_dev = has_both || self.dev;

        let mut base_install = state.install(
            std::iter::empty()
                .chain(include_prod.then_some(DependencyGroup::Prod))
                .chain(include_dev.then_some(DependencyGroup::Dev))
                .chain(fetch_config.optional.then_some(DependencyGroup::Optional)),
        );
        base_install.context.config = fetch_config;
        base_install.lockfile_policy.frozen = true;
        base_install.lockfile_policy.ignore_manifest_check = true;
        base_install.execution.mutation = ProjectMutation::Fetch;
        base_install.context.lockfile_path = Some(&lockfile_path);
        match selection.map(workspace_install_selection) {
            Some(selection) => base_install.run_selected::<Reporter>(selection).await,
            None => base_install.run::<Reporter>().await,
        }
        .wrap_err("fetching dependencies")
    }
}
