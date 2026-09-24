use std::{collections::HashSet, path::PathBuf};

use futures_util::{StreamExt, TryStreamExt, stream};
use pnpm_config::Config;
use pnpm_publish::{
    PublishNetwork, PublishPackedPkgOptions, find_registry_info, publish_config_registry,
    wait_for_published_packages,
};
use pnpm_reporter::Reporter;

use super::{PublishArgs, recursive::publish_eligible};

impl PublishArgs {
    pub(super) async fn wait_for_existing_projects<Reporter: self::Reporter>(
        &self,
        graph: &pnpm_workspace_projects_filter::ProjectGraph<pnpm_workspace::GraphPkg<'_>>,
        to_publish: &HashSet<PathBuf>,
        config: &Config,
        opts: &PublishPackedPkgOptions,
        network: &PublishNetwork<'_>,
    ) -> miette::Result<()> {
        if opts.dry_run || opts.wait_timeout.is_zero() {
            return Ok(());
        }
        let checks = graph
            .iter()
            .filter_map(|(root, node)| {
                let manifest = node.package.project.manifest.value();
                let (name, version) = publish_eligible(manifest)?;
                if to_publish.contains(root) {
                    return None;
                }
                Some(async move {
                    let registry = find_registry_info(
                        name,
                        &config.registry,
                        &config.registries_by_scope,
                        publish_config_registry(manifest, name),
                    )?;
                    wait_for_published_packages::<Reporter>(
                        &[(name, version)],
                        &registry,
                        network,
                        opts.wait_timeout,
                    )
                    .await?;
                    Ok::<_, miette::Report>(())
                })
            })
            .collect::<Vec<_>>();
        stream::iter(checks).buffer_unordered(4).try_collect().await
    }
}
