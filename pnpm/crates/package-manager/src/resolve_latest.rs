//! Maturity-aware resolution of a package's `latest` dist-tag.
//!
//! `pacquet add <name>` (no version) and `pacquet update --latest` write the
//! version behind the `latest` tag into `package.json`. Resolving that tag
//! through the same picker the install uses means an active
//! `minimumReleaseAge` repoints `latest` to the newest mature version instead
//! of the raw dist-tag, so the manifest never gets a range the follow-up
//! install would reject
//! ([pnpm/pnpm#11165](https://github.com/pnpm/pnpm/issues/11165)).

use crate::resolution_policy::{PickPolicy, pick_package_context};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_network::ThrottledClient;
use pacquet_registry::PackageVersion;
use pacquet_resolving_npm_resolver::{
    InMemoryPackageMetaCache, PackumentFetchLocker, PickPackageError, PickPackageOptions,
    RegistryPackageSpec, pick_package, pick_registry_for_package, shared_packument_fetch_locker,
};
use std::{collections::HashMap, sync::Arc};

/// Error type of the crate-internal `LatestPicker::resolve`.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ResolveLatestError {
    #[diagnostic(transparent)]
    Pick(#[error(source)] Box<PickPackageError>),

    /// The packument carries no version behind the `latest` tag (nor a
    /// fallback pick) — e.g. every version was unpublished.
    #[display("no version found for the latest tag")]
    #[diagnostic(code(pacquet_package_manager::no_latest_version))]
    NoLatestVersion,
}

/// Maturity-aware picker for `latest` dist-tags (see the module docs for
/// why). One instance per command run: every [`Self::resolve`] call shares
/// the policy's single `minimumReleaseAge` cutoff instant plus one metadata
/// cache, fetch locker, and registries map, so an `update --latest` run
/// resolves all matched dependencies against the same view instead of
/// re-fetching per call.
pub(crate) struct LatestPicker<'a> {
    config: &'a Config,
    http_client: &'a ThrottledClient,
    policy: PickPolicy,
    meta_cache: InMemoryPackageMetaCache,
    fetch_locker: PackumentFetchLocker,
    registries: HashMap<String, String>,
}

impl<'a> LatestPicker<'a> {
    pub(crate) fn new(
        config: &'a Config,
        http_client: &'a ThrottledClient,
        policy: PickPolicy,
    ) -> Self {
        Self {
            config,
            http_client,
            policy,
            meta_cache: InMemoryPackageMetaCache::default(),
            fetch_locker: shared_packument_fetch_locker(),
            registries: config.resolved_registries().into_iter().collect(),
        }
    }

    /// Resolve `package_name`'s `latest` dist-tag to a concrete version
    /// through the same maturity-aware picker the install uses.
    ///
    /// `dry_run` skips the metadata cache write-back (`--lockfile-only`).
    pub(crate) async fn resolve(
        &self,
        package_name: &str,
        dry_run: bool,
    ) -> Result<Arc<PackageVersion>, ResolveLatestError> {
        let registry = pick_registry_for_package(&self.registries, package_name, None);
        let spec = RegistryPackageSpec::latest_tag(package_name);

        let opts = PickPackageOptions {
            registry: &registry,
            preferred_version_selectors: None,
            published_by: self.policy.published_by,
            published_by_exclude: self.policy.published_by_exclude.as_ref(),
            pick_lowest_version: false,
            // The spec already is the `latest` tag.
            include_latest_tag: false,
            dry_run,
            optional: false,
            update_checksums: false,
            blocked_versions: None,
        };
        let ctx = pick_package_context(
            self.http_client,
            self.config,
            &self.policy,
            &self.meta_cache,
            &self.fetch_locker,
        );

        let pick = pick_package(&ctx, &spec, &opts)
            .await
            .map_err(|error| ResolveLatestError::Pick(Box::new(error)))?;
        pick.picked_package.ok_or(ResolveLatestError::NoLatestVersion)
    }
}
