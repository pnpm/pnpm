use crate::{State, cli_args::install::resolve_bool_override};
use clap::Args;
use miette::Context;
use pnpm_package_manager::Install;
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::Reporter;

#[derive(Debug, Args)]
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

    pub async fn run<Reporter: self::Reporter + 'static>(self, state: State) -> miette::Result<()> {
        let lockfile_path = state.lockfile_path();
        let State { tarball_mem_cache, http_client, config, manifest, lockfile, resolved_packages } =
            &state;

        let dependency_groups: Vec<DependencyGroup> =
            self.dependency_groups(config.optional).collect();

        Install {
            lockfile_path: Some(&lockfile_path),
            skip_runtimes: false,
            trust_lockfile: false,
            disable_optimistic_repeat_install: true,
            ..Install::new(
                std::sync::Arc::clone(tarball_mem_cache),
                resolved_packages,
                (http_client, std::sync::Arc::clone(http_client)),
                config,
                manifest,
                pnpm_lockfile::MaybeLazyLockfile::Lazy(lockfile),
                dependency_groups,
            )
        }
        .run::<Reporter>()
        .await
        .wrap_err("pruning dependencies")
    }
}
