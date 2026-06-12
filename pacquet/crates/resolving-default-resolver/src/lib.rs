//! Pacquet port of pnpm's
//! [`@pnpm/resolving.default-resolver`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/default-resolver/src/index.ts).
//!
//! The dispatcher: composes a heterogeneous list of [`Resolver`]s into
//! a single chain that the deps-resolver calls per wanted dependency.
//! Each resolver in the chain returns `Ok(None)` to defer to the next
//! one and `Ok(Some(_))` to claim the wanted dependency.
//!
//! Today the chain is empty until the per-protocol resolvers
//! (npm/jsr/git/tarball/local/runtimes/named-registry/workspace) land
//! in subsequent PRs. A [`DefaultResolver`] built without any
//! resolvers always returns [`SpecNotSupportedByAnyResolverError`],
//! mirroring pnpm's
//! [`SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/default-resolver/src/index.ts#L152-L156)
//! error code.

use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_resolving_resolver_base::{
    LatestInfo, LatestQuery, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions,
    ResolveResult, Resolver, WantedDependency,
};

/// Composed-chain analog of pnpm's
/// [`createResolver`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/default-resolver/src/index.ts#L97-L173)
/// return value. Wraps an ordered list of per-protocol resolvers.
///
/// Order matters: each resolver in the chain gets the chance to claim
/// the wanted dependency in declaration order, mirroring the `??`
/// chain upstream uses inside `createResolver`. Wiring of the actual
/// resolvers (npm, jsr, git, tarball, local, runtimes, named-registry,
/// workspace) lands in subsequent PRs as each per-protocol crate is
/// ported.
pub struct DefaultResolver {
    chain: Vec<Box<dyn Resolver>>,
}

impl DefaultResolver {
    /// Build a dispatcher from a chain of resolvers. Order is preserved
    /// — earlier entries get the first shot at every wanted dependency.
    #[must_use]
    pub fn new(chain: Vec<Box<dyn Resolver>>) -> Self {
        Self { chain }
    }

    /// Walk the chain and return the first resolver's claim. Returns
    /// [`SpecNotSupportedByAnyResolverError`] when no resolver claims
    /// the wanted dependency, matching pnpm's
    /// [`SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/default-resolver/src/index.ts#L152-L156).
    pub async fn resolve(
        &self,
        wanted_dependency: &WantedDependency,
        opts: &ResolveOptions,
    ) -> Result<ResolveResult, ResolveError> {
        for resolver in &self.chain {
            if let Some(result) = resolver.resolve(wanted_dependency, opts).await? {
                return Ok(result);
            }
        }
        Err(Box::new(SpecNotSupportedByAnyResolverError::new(wanted_dependency)))
    }

    /// Latest-version companion to [`Self::resolve`]. Upstream's
    /// [`resolveLatest`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/default-resolver/src/index.ts#L159-L170)
    /// returns `undefined` (no resolver had an opinion) rather than
    /// erroring — pacquet mirrors that by returning `Ok(None)` once
    /// the chain is exhausted.
    pub async fn resolve_latest(
        &self,
        query: &LatestQuery,
        opts: &ResolveOptions,
    ) -> Result<Option<LatestInfo>, ResolveError> {
        for resolver in &self.chain {
            if let Some(info) = resolver.resolve_latest(query, opts).await? {
                return Ok(Some(info));
            }
        }
        Ok(None)
    }
}

/// `DefaultResolver` doubles as a [`Resolver`] so callers can compose
/// it into another dispatcher (or hand it to a consumer that already
/// accepts the trait, like `resolve_dependency_tree`). Through the
/// trait, the "no resolver claimed" branch surfaces as `Ok(None)` so
/// the caller chooses how to react — the inherent
/// [`Self::resolve`](DefaultResolver::resolve) method keeps raising
/// [`SpecNotSupportedByAnyResolverError`] for callers that prefer the
/// error form.
impl Resolver for DefaultResolver {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        Box::pin(async move {
            for resolver in &self.chain {
                if let Some(result) = resolver.resolve(wanted_dependency, opts).await? {
                    return Ok(Some(result));
                }
            }
            Ok(None)
        })
    }

    fn resolve_latest<'a>(
        &'a self,
        query: &'a LatestQuery,
        opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(self.resolve_latest(query, opts))
    }
}

/// The `SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER` error code raised when
/// every resolver in the chain returned `Ok(None)` for a wanted
/// dependency.
///
/// Message format matches upstream's
/// [`createResolver` error path](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/default-resolver/src/index.ts#L148-L156):
/// the offending specifier is rendered as `<alias>@<bareSpecifier>`
/// (either half omitted when absent) and quoted when non-empty.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("{quoted} isn't supported by any available resolver.")]
#[diagnostic(code(SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER))]
pub struct SpecNotSupportedByAnyResolverError {
    /// Quoted offending specifier, formatted upstream-style at
    /// construction so the `Display` impl stays allocation-free.
    /// Empty string when both halves of the wanted dependency are
    /// absent (matches the upstream branch that drops the quotes for
    /// the empty case).
    pub quoted: String,
    /// Unquoted form of the same specifier — `<alias>@<bareSpecifier>`
    /// with either half omitted when absent. Kept separately so
    /// callers and tests can read the bare value without re-parsing
    /// the formatted message.
    pub specifier: String,
}

impl SpecNotSupportedByAnyResolverError {
    #[must_use]
    pub fn new(wanted_dependency: &WantedDependency) -> Self {
        let specifier = render_specifier(wanted_dependency);
        let quoted = quote_specifier(&specifier);
        Self { quoted, specifier }
    }
}

/// Format the offending specifier the way upstream does:
/// `<alias>@<bareSpecifier>` with either half omitted when absent.
/// Used at error-construction time so the message is computed once.
fn render_specifier(wanted_dependency: &WantedDependency) -> String {
    let alias = wanted_dependency.alias.as_deref().unwrap_or("");
    let bare = wanted_dependency.bare_specifier.as_deref().unwrap_or("");
    if alias.is_empty() && bare.is_empty() {
        return String::new();
    }
    if alias.is_empty() {
        return bare.to_string();
    }
    if bare.is_empty() {
        return alias.to_string();
    }
    format!("{alias}@{bare}")
}

/// Wrap a non-empty specifier in double quotes and leave the empty
/// case bare. Mirrors upstream's
/// ``if (specifier !== '') specifier = `"${specifier}"` `` step.
fn quote_specifier(specifier: &str) -> String {
    if specifier.is_empty() { String::new() } else { format!("\"{specifier}\"") }
}

#[cfg(test)]
mod tests;
