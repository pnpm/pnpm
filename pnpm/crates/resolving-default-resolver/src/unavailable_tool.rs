use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_resolving_resolver_base::{
    LatestQuery, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions, Resolver,
    WantedDependency,
};
use pnpm_tool_protocol::is_tool_specifier;

/// Raised by [`UnavailableToolResolver`] for a `runtime:` or `tool:` request.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("Runtime provisioning is not enabled in this build for {alias}@{specifier}")]
#[diagnostic(code(ERR_PNPM_RUNTIME_PROVISIONING_UNAVAILABLE))]
pub struct RuntimeProvisioningUnavailableError {
    pub alias: String,
    pub specifier: String,
}

/// Stands in for the node, deno, bun, and yarn resolvers in a chain built
/// without the `runtime-provisioning` feature. It must come before the
/// named-registry and local-path resolvers, which would otherwise claim or
/// misreport the request.
pub struct UnavailableToolResolver;

impl Resolver for UnavailableToolResolver {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        Box::pin(async move {
            let Some(alias) = wanted_dependency.alias.as_deref() else { return Ok(None) };
            let Some(specifier) = wanted_dependency.bare_specifier.as_deref() else {
                return Ok(None);
            };
            if !matches!(alias, "node" | "deno" | "bun" | "yarn") || !is_tool_specifier(specifier) {
                return Ok(None);
            }
            Err(Box::new(RuntimeProvisioningUnavailableError {
                alias: alias.to_string(),
                specifier: specifier.to_string(),
            }) as ResolveError)
        })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async { Ok(None) })
    }
}

#[cfg(test)]
mod tests;
