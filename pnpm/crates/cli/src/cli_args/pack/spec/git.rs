//! `pack <git spec>`: check the repository out, prepare it the way an
//! install prepares a git dependency, and pack the prepared directory.
//!
//! The named package may run its build scripts without an `allowBuilds`
//! entry, as a package named on the `pnpm dlx` command line may:
//! naming it is the approval. `--ignore-scripts` still skips them.

use super::{
    PackDir,
    pack_dir,
    redact_spec,
};
use crate::cli_args::{
    pack::PackArgs,
    registry_client::build_registry_client,
};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_git_fetcher::{
    GitSource,
    GitSourceCache,
    PrepareGitCheckout,
    PreparedCheckout,
};
use pnpm_hooks::PnpmfileHooks;
use pnpm_lockfile::{
    GitResolution,
    LockfileResolution,
};
use pnpm_pack::PackResult;
use pnpm_reporter::Reporter;
use pnpm_resolving_git_resolver::{
    GitFetchContext,
    GitProbe,
    GitResolver,
    ProbeFuture,
    RealGitRunner,
    create_git_hosted_pkg_id,
};
use pnpm_resolving_resolver_base::{
    ResolveOptions,
    ResolveResult,
    Resolver,
    WantedDependency,
};
use std::{
    path::Path,
    sync::Arc,
};

/// Raised if a git spec does not resolve to a commit. `classify` admits
/// only git specs here, and [`CheckoutOnly`] rules out a host archive, so
/// this marks a broken invariant rather than bad input.
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
#[display("Cannot pack \"{spec}\": it did not resolve to a git commit")]
#[diagnostic(code(ERR_PNPM_PACK_UNSUPPORTED_SPEC))]
struct NoGitCheckout {
    #[error(not(source))]
    spec: String,
}

pub(super) async fn pack_git_spec<Reporter: self::Reporter>(
    args: &PackArgs,
    dir: &Path,
    config: &Config,
    before_packing_hooks: &[Arc<dyn PnpmfileHooks>],
    spec: &str,
) -> miette::Result<PackResult> {
    let source_cache = Arc::new(GitSourceCache::default());
    let resolved = resolve_git_spec(config, &source_cache, spec).await?;
    let LockfileResolution::Git(git) = &resolved.resolution else {
        return Err(NoGitCheckout { spec: redact_spec(spec) }.into());
    };
    let package_name = resolved.package.manifest
        .as_deref()
        .and_then(|manifest| manifest.get("name"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| redact_spec(spec));
    let PreparedCheckout { checkout, prepared } = tokio::task::block_in_place(|| {
        prepare_checkout::<Reporter>(config, &source_cache, git, &package_name)
    })?;
    let packed = pack_dir::<Reporter>(
        args,
        dir,
        config,
        before_packing_hooks,
        PackDir { path: prepared.pkg_dir, in_workspace: false, scripts_already_ran: true },
    )
    .await;
    drop(checkout);
    packed
}

/// Pin the spec to a commit and read the package's manifest. The checkout
/// this makes lands in `source_cache`, so preparing reuses it.
async fn resolve_git_spec(
    config: &Config,
    source_cache: &Arc<GitSourceCache>,
    spec: &str,
) -> miette::Result<ResolveResult> {
    let http_client = Arc::new(build_registry_client(config)?);
    let resolver = GitResolver::new(
        Arc::new(CheckoutOnly),
        Arc::new(RealGitRunner::new().with_connect_guard(config.auth_headers.connect_guard())),
    )
    .with_fetch_context(GitFetchContext {
        source_cache: Arc::clone(source_cache),
        http_client,
        store_dir: std::borrow::Cow::Owned(config.store_dir.clone()),
        store_index_writer: None,
        auth_headers: Arc::clone(&config.auth_headers),
        retry_opts: config.retry_opts(),
        tar_limits: pnpm_tarball::TarLimits::new(config.max_tarball_unpacked_size),
        git_shallow_hosts: config.git_shallow_hosts.clone(),
    });
    let wanted =
        WantedDependency { bare_specifier: Some(spec.to_string()), ..WantedDependency::default() };
    resolver
        .resolve(&wanted, &ResolveOptions::default())
        .await
        .map_err(pnpm_resolving_deps_resolver::ResolveDependencyTreeError::from_resolve_error)?
        .ok_or_else(|| NoGitCheckout { spec: redact_spec(spec) }.into())
}

fn prepare_checkout<Reporter: self::Reporter>(
    config: &Config,
    source_cache: &GitSourceCache,
    git: &GitResolution,
    package_name: &str,
) -> Result<PreparedCheckout, pnpm_git_fetcher::GitFetcherError> {
    let package_id = create_git_hosted_pkg_id(&git.repo, &git.commit, git.path.as_deref());
    let allow_named_package = |_: &str| Some(true);
    PrepareGitCheckout {
        scripts: pnpm_deps_restorer::install_package_by_snapshot::git_prepare_script_options(
            config,
        ),
        source: GitSource {
            cache: source_cache,
            repo: &git.repo,
            commit: &git.commit,
            path: git.path.as_deref(),
            shallow_hosts: &config.git_shallow_hosts,
            git_bin: None,
        },
        allow_build: &allow_named_package,
        package_id: &package_id,
        package_name,
    }
    .run::<Reporter>()
}

/// Reports no host archive as fetchable, so every spec resolves to a
/// checkout: the package is prepared from a working tree.
struct CheckoutOnly;

impl GitProbe for CheckoutOnly {
    fn anonymous_head_ok<'a>(&'a self, _url: &'a str) -> ProbeFuture<'a> {
        Box::pin(std::future::ready(false))
    }
}
