use super::ResolverChainInputs;
use pnpm_engine_pm_yarn_resolver::YarnResolver;
use pnpm_engine_runtime_bun_resolver::BunResolver;
use pnpm_engine_runtime_deno_resolver::DenoResolver;
use pnpm_resolving_resolver_base::Resolver;
use std::sync::Arc;

pub(super) fn runtime_resolvers(
    inputs: &ResolverChainInputs<'_>,
    npm_resolver: &Arc<dyn Resolver>,
) -> Vec<Box<dyn Resolver>> {
    vec![
        Box::new(inputs.node_resolver()),
        Box::new(DenoResolver::new(
            Arc::clone(inputs.fetching.http_client),
            Arc::clone(npm_resolver),
        )),
        Box::new(
            BunResolver::new(Arc::clone(inputs.fetching.http_client), Arc::clone(npm_resolver))
                .with_mirror(inputs.config.tool_mirror(pnpm_config::Tool::Bun)),
        ),
        Box::new(YarnResolver::new(
            Arc::clone(inputs.fetching.http_client),
            inputs.config.tls.strict_ssl.unwrap_or(true),
        )),
    ]
}
