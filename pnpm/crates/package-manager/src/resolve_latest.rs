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
use pnpm_config::Config;
use pnpm_network::{ThrottledClient, redact_and_sanitize};
use pnpm_registry::{PackageTag, PackageVersion};
use pnpm_resolving_npm_resolver::{
    InMemoryPackageMetaCache, PackumentFetchLocker, PickPackageError, PickPackageOptions,
    RegistryPackageSpec, pick_package, pick_registry_for_package,
};
use std::{collections::HashMap, sync::Arc};

/// Error type of the crate-internal `LatestPicker::resolve`.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ResolveLatestError {
    #[diagnostic(transparent)]
    Pick(#[error(source)] Box<PickPackageError>),

    #[diagnostic(transparent)]
    Registry(#[error(source)] pnpm_registry::RegistryError),

    /// The packument carries no version behind the `latest` tag (nor a
    /// fallback pick) — e.g. every version was unpublished.
    #[display("no version found for the latest tag")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_NO_LATEST_VERSION))]
    NoLatestVersion,

    /// The `latest` tag names a version the packument lists but whose
    /// manifest pnpm could not decode. A version that fails to decode is
    /// skipped as if the registry never served it, so without this the
    /// failure surfaces as an empty `latest` tag and points the user at
    /// the wrong thing entirely.
    ///
    /// The guidance lives in the message rather than a `help(..)`
    /// because every caller wraps this behind its own diagnostic code,
    /// which drops the inner help before it reaches the terminal.
    ///
    /// `version` and `error` reproduce registry-controlled text, so both
    /// are sanitized on the way in.
    #[display(
        "the registry served a manifest for {name}@{version} that pnpm could not read, so the version was skipped: {error}"
    )]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_UNDECODABLE_LATEST_MANIFEST))]
    UndecodableLatestManifest { name: String, version: String, error: String },
}

/// Maturity-aware picker for `latest` dist-tags (see the module docs for
/// why). One instance per command run: every [`Self::resolve`] call shares
/// the policy's single `minimumReleaseAge` cutoff instant and registries map,
/// plus one metadata cache and fetch locker whenever policy-aware resolution
/// needs the packument.
pub(crate) struct LatestPicker<'a> {
    config: &'a Config,
    http_client: &'a ThrottledClient,
    policy: PickPolicy,
    meta_cache: Arc<InMemoryPackageMetaCache>,
    fetch_locker: PackumentFetchLocker,
    registries: HashMap<String, String>,
}

impl<'a> LatestPicker<'a> {
    pub(crate) fn new(
        config: &'a Config,
        http_client: &'a ThrottledClient,
        policy: PickPolicy,
        meta_cache: Arc<InMemoryPackageMetaCache>,
        fetch_locker: PackumentFetchLocker,
    ) -> Self {
        Self {
            config,
            http_client,
            policy,
            meta_cache,
            fetch_locker,
            registries: config.resolved_registries().into_iter().collect(),
        }
    }

    /// Resolve `package_name`'s `latest` dist-tag to a concrete version. The
    /// default online path keeps the lightweight dist-tag endpoint; maturity
    /// filtering and offline modes use the install-equivalent package picker.
    ///
    /// `dry_run` skips the metadata cache write-back (`--lockfile-only`).
    pub(crate) async fn resolve(
        &self,
        package_name: &str,
        dry_run: bool,
    ) -> Result<Arc<PackageVersion>, ResolveLatestError> {
        let registry = pick_registry_for_package(&self.registries, package_name, None);
        if self.policy.published_by.is_none()
            && self.policy.published_by_exclude.is_none()
            && !self.config.offline
            && !self.config.prefer_offline
        {
            return PackageVersion::fetch_from_registry(
                package_name,
                PackageTag::Latest,
                self.http_client,
                &registry,
                &self.config.auth_headers,
            )
            .await
            .map(Arc::new)
            .map_err(ResolveLatestError::Registry);
        }
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
            trust_policy: Some(self.config.trust_policy),
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
        if let Some(picked) = pick.picked_package {
            return Ok(picked);
        }
        let Some((version, error)) = pick.meta.latest_decode_error() else {
            return Err(ResolveLatestError::NoLatestVersion);
        };
        Err(ResolveLatestError::UndecodableLatestManifest {
            name: package_name.to_string(),
            version: redact_and_sanitize(version),
            error: redact_and_sanitize(&error),
        })
    }
}

#[cfg(test)]
mod tests;
