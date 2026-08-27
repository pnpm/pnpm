//! Maturity-aware resolution of a package's `latest` dist-tag.
//!
//! `pacquet add <name>` (no version) and `pacquet update --latest` write the
//! version behind the `latest` tag into `package.json`. Resolving that tag
//! through the same picker the install uses means an active
//! `minimumReleaseAge` repoints `latest` to the newest mature version instead
//! of the raw dist-tag, so the manifest never gets a range the follow-up
//! install would reject
//! ([pnpm/pnpm#11165](https://github.com/pnpm/pnpm/issues/11165)).
//!
//! A version being old enough is not on its own enough for the install to
//! accept it: a release that pins its platform binaries to an exact version
//! it did not publish at the same moment is itself mature while the versions
//! it requires are not, and the install has no way to satisfy the pin. Those
//! candidates are rejected here too, and the next version down is offered
//! instead, so the same reasoning covers both halves of "a range the install
//! would reject" ([pnpm/pnpm#11068](https://github.com/pnpm/pnpm/issues/11068)).
//!
//! Only exact pins are checked. A candidate's ranged dependencies can be
//! satisfied by any mature version the range admits, and deciding that is
//! the install's resolution — which backs out of the parent that declared
//! the edge when no such version exists. A pin has no such freedom, which is
//! why it is the half worth answering before the manifest is written.

use crate::resolution_policy::{PickPolicy, pick_package_context};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::{Config, version_policy::PolicyMatch};
use pnpm_network::ThrottledClient;
use pnpm_registry::{PackageTag, PackageVersion};
use pnpm_resolving_npm_resolver::{
    InMemoryPackageMetaCache, PackumentFetchLocker, PickPackageError, PickPackageOptions,
    RegistryPackageSpec, RegistryPackageSpecType, pick_package, pick_registry_for_package,
};
use pnpm_resolving_resolver_base::{
    PackageVersionGuard, PackageVersionGuardDecision, PackageVersionGuardFuture,
    parse_packument_timestamp,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

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
}

/// How far down the version list a candidate search walks before taking
/// whatever it has. Each step costs one packument read per exact pin, and a
/// package with a long run of releases that all pin something too young is
/// better served by the install's own error than by an unbounded search.
const MAX_REJECTED_CANDIDATES: usize = 8;

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

        let mut rejected: HashSet<String> = HashSet::new();
        loop {
            let opts = PickPackageOptions {
                blocked_versions: (!rejected.is_empty()).then_some(&rejected),
                ..opts
            };
            let pick = pick_package(&ctx, &spec, &opts)
                .await
                .map_err(|error| ResolveLatestError::Pick(Box::new(error)))?;
            let candidate = pick.picked_package.ok_or(ResolveLatestError::NoLatestVersion)?;
            if rejected.len() >= MAX_REJECTED_CANDIDATES
                || self.pins_only_installable_versions(&candidate, dry_run).await?
            {
                return Ok(candidate);
            }
            rejected.insert(candidate.version.to_string());
        }
    }

    /// [`Self::pins_only_installable_versions`] for a package named by
    /// `name@version` rather than by an already-picked manifest.
    ///
    /// Errors when that exact version cannot be read back from the registry;
    /// the caller decides what an unreadable candidate means.
    pub(crate) async fn pins_installable_for(
        &self,
        name: &str,
        version: &str,
        dry_run: bool,
    ) -> Result<bool, ResolveLatestError> {
        if self.policy.published_by.is_none() {
            return Ok(true);
        }
        let registry = pick_registry_for_package(&self.registries, name, None);
        let spec = RegistryPackageSpec {
            name: name.to_string(),
            fetch_spec: version.to_string(),
            spec_type: RegistryPackageSpecType::Version,
            revision: None,
            normalized_bare_specifier: None,
        };
        let opts = PickPackageOptions {
            registry: &registry,
            preferred_version_selectors: None,
            // The named version is what the caller is judging, so it must
            // come back whatever the cutoff says about it.
            published_by: None,
            published_by_exclude: None,
            pick_lowest_version: false,
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
        let candidate = pick.picked_package.ok_or(ResolveLatestError::NoLatestVersion)?;
        self.pins_only_installable_versions(&candidate, dry_run).await
    }

    /// Whether every version `candidate` pins exactly is itself old enough to
    /// install under the cutoff.
    ///
    /// `true` whenever no cutoff is configured, and for a pin the cutoff does
    /// not reach: a package `minimumReleaseAgeExclude` covers is installable
    /// at any age, so it cannot be the reason to pass this candidate over.
    ///
    /// A pin whose packument cannot be read, or that carries no publish time,
    /// also answers `true`. This runs before an install that will resolve the
    /// same package and report the real failure; refusing a candidate on a
    /// metadata gap would trade a clear error for a silent downgrade.
    async fn pins_only_installable_versions(
        &self,
        candidate: &PackageVersion,
        dry_run: bool,
    ) -> Result<bool, ResolveLatestError> {
        let Some(cutoff) = self.policy.published_by else { return Ok(true) };
        for (name, pinned) in exact_pins(candidate) {
            if self
                .policy
                .published_by_exclude
                .as_ref()
                .is_some_and(|policy| policy.matches(name) != PolicyMatch::No)
            {
                continue;
            }
            let registry = pick_registry_for_package(&self.registries, name, None);
            let opts = PickPackageOptions {
                registry: &registry,
                preferred_version_selectors: None,
                // The cutoff is passed so the packument arrives in its full
                // form: the per-version `time` this reads only comes with
                // it, and asking without a cutoff leaves an abbreviated
                // document whose missing times read as "old enough".
                // Narrowing the versions is harmless — `time` survives it,
                // and the pick itself is discarded.
                published_by: self.policy.published_by,
                published_by_exclude: self.policy.published_by_exclude.as_ref(),
                pick_lowest_version: false,
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
            let spec = RegistryPackageSpec {
                name: name.to_string(),
                fetch_spec: pinned.to_string(),
                spec_type: RegistryPackageSpecType::Version,
                revision: None,
                normalized_bare_specifier: None,
            };
            let Ok(pick) = pick_package(&ctx, &spec, &opts).await else { continue };
            let Some(time) = pick.meta.time.as_ref() else { continue };
            let Some(published_at) = time
                .get(pinned)
                .and_then(serde_json::Value::as_str)
                .and_then(parse_packument_timestamp)
            else {
                continue;
            };
            if published_at > cutoff {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// The `name -> version` pairs a manifest pins to one exact version, across
/// the dependency groups an install has to satisfy. `optionalDependencies` is
/// included because a lockfile records every platform's binary, so an
/// immature one blocks the install on every platform, not just its own.
fn exact_pins(candidate: &PackageVersion) -> impl Iterator<Item = (&str, &str)> {
    candidate
        .dependencies
        .iter()
        .chain(candidate.optional_dependencies.iter())
        .flatten()
        .filter_map(|(name, spec)| {
            node_semver::Version::parse(spec).is_ok().then_some((name.as_str(), spec.as_str()))
        })
}

/// [`PackageVersionGuard`] form of the pin check.
///
/// `pacquet update --latest` picks its target through the whole resolver
/// chain rather than [`LatestPicker`], so the same question reaches it
/// through the guard that chain already consults: a rejection makes the
/// picker exclude that version and offer the next one down, which is the
/// walk [`LatestPicker::resolve`] runs for itself.
///
/// The chain resolves one declared dependency per call, so the guard only
/// ever judges the package being updated — never the graph below it, which
/// is the install's to resolve.
pub(crate) struct MaturePinsGuard {
    /// Owned rather than borrowed: the guard is handed to the resolver as an
    /// `Arc<dyn PackageVersionGuard>`, which outlives any borrow the command
    /// has of its own config.
    config: Arc<Config>,
    http_client: Arc<ThrottledClient>,
    policy: PickPolicy,
    meta_cache: Arc<InMemoryPackageMetaCache>,
    fetch_locker: PackumentFetchLocker,
    dry_run: bool,
}

impl MaturePinsGuard {
    pub(crate) fn new(
        config: &Config,
        http_client: Arc<ThrottledClient>,
        policy: PickPolicy,
        dry_run: bool,
    ) -> Self {
        Self {
            config: Arc::new(config.clone()),
            http_client,
            policy,
            meta_cache: Arc::new(InMemoryPackageMetaCache::default()),
            fetch_locker: PackumentFetchLocker::default(),
            dry_run,
        }
    }
}

impl std::fmt::Debug for MaturePinsGuard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MaturePinsGuard")
            .field("published_by", &self.policy.published_by)
            .field("dry_run", &self.dry_run)
            .finish_non_exhaustive()
    }
}

impl PackageVersionGuard for MaturePinsGuard {
    fn check<'a>(&'a self, name: &'a str, version: &'a str) -> PackageVersionGuardFuture<'a> {
        Box::pin(async move {
            let picker = LatestPicker::new(
                &self.config,
                &self.http_client,
                self.policy.clone(),
                Arc::clone(&self.meta_cache),
                Arc::clone(&self.fetch_locker),
            );
            Ok(match picker.pins_installable_for(name, version, self.dry_run).await {
                Ok(false) => PackageVersionGuardDecision::Reject {
                    reason: format!(
                        "{name}@{version} depends on a version that minimumReleaseAge does not \
                         admit yet, and pins it exactly",
                    ),
                },
                // A candidate whose own metadata cannot be read is left to
                // the install, which resolves it next and reports the real
                // failure. See `pins_only_installable_versions`.
                Ok(true) | Err(_) => PackageVersionGuardDecision::Allow,
            })
        })
    }
}

#[cfg(test)]
mod tests;
