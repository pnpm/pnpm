use crate::{
    State,
    cli_args::{
        install::{resolve_bool_override, workspace_install_selection},
        pipelines::InstallFamilySelection,
    },
};
use clap::Args;
use miette::Context;
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::Reporter;

#[derive(Debug, Clone, Args)]
pub struct PruneArgs {
    #[clap(short = 'P', long, visible_alias = "production")]
    prod: bool,
    #[clap(short = 'D', long)]
    dev: bool,
    #[clap(long, overrides_with = "no_optional")]
    optional: bool,
    #[clap(long, overrides_with = "optional")]
    no_optional: bool,
    #[clap(long = "ignore-scripts", overrides_with = "no_ignore_scripts")]
    pub ignore_scripts: bool,
    /// Run lifecycle scripts even if scripts are disabled by configuration.
    #[clap(long = "no-ignore-scripts", overrides_with = "ignore_scripts")]
    pub no_ignore_scripts: bool,
}

impl PruneArgs {
    fn dependency_groups(&self, include_optional: bool) -> impl Iterator<Item = DependencyGroup> {
        let &PruneArgs {
            prod,
            dev,
            optional,
            no_optional,
            ignore_scripts: _,
            no_ignore_scripts: _,
        } = self;
        let has_both = prod == dev;
        let has_prod = has_both || prod;
        let has_dev = has_both || dev;
        let has_optional = resolve_bool_override(optional, no_optional, include_optional);
        std::iter::empty()
            .chain(has_prod.then_some(DependencyGroup::Prod))
            .chain(has_dev.then_some(DependencyGroup::Dev))
            .chain(has_optional.then_some(DependencyGroup::Optional))
    }

    pub(crate) async fn run<Reporter: self::Reporter + 'static>(
        self,
        state: State,
        selection: Option<&InstallFamilySelection>,
    ) -> miette::Result<()> {
        let lockfile_path = state.lockfile_path();
        let dependency_groups: Vec<DependencyGroup> =
            self.dependency_groups(state.config.optional).collect();
        let selection = selection.map(|sel| {
            let mut ws_selection = workspace_install_selection(sel);
            ws_selection.install_dirs = sel.selected_dirs.as_ref();
            ws_selection
        });

        {
            let mut base_install = state.install(dependency_groups);
            base_install.lockfile_policy.trust = state.config.trust_lockfile;
            base_install.lockfile_policy.disable_optimistic_repeat = true;
            base_install.context.lockfile_path = Some(&lockfile_path);
            base_install
        }
        .run_prune::<Reporter>(selection)
        .await
        .wrap_err("pruning dependencies")
    }
}
