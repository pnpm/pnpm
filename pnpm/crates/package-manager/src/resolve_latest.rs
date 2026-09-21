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
use futures_util::{StreamExt, stream};
use miette::Diagnostic;
use pnpm_config::{
    Config,
    version_policy::{PackageVersionPolicy, PolicyMatch},
};
use pnpm_network::{ThrottledClient, redact_and_sanitize};
use pnpm_registry::{PackageTag, PackageVersion};
use pnpm_resolving_npm_resolver::{
    GUARD_REPICK_LIMIT, InMemoryPackageMetaCache, PackumentFetchLocker, PickPackageError,
    PickPackageOptions, RegistryPackageSpec, RegistryPackageSpecType, blocked_packument_key,
    parse_bare_specifier, parse_named_registry_specifier_to_registry_package_spec, pick_package,
    pick_registry_for_package,
};
use pnpm_resolving_resolver_base::{
    PackageVersionGuard, PackageVersionGuardDecision, PackageVersionGuardFuture,
    parse_packument_timestamp,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
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
    registry_names: HashSet<String>,
    registries_by_prefix: BTreeMap<String, String>,
}

impl<'a> LatestPicker<'a> {
    pub(crate) fn new(
        config: &'a Config,
        http_client: &'a ThrottledClient,
        policy: PickPolicy,
        meta_cache: Arc<InMemoryPackageMetaCache>,
        fetch_locker: PackumentFetchLocker,
    ) -> Self {
        let registries_by_prefix = config.resolved_registry_lookups().registries_by_prefix;
        Self {
            config,
            http_client,
            policy,
            meta_cache,
            fetch_locker,
            registry_names: registries_by_prefix
                .keys()
                .cloned()
                .collect(),
            registries_by_prefix,
            registries: config
                .resolved_registries()
                .into_iter()
                .collect(),
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
        self.pick_latest(package_name, dry_run, &registry).await
    }

    fn pick_options<'options>(
        &'options self,
        registry: &'options str,
        dry_run: bool,
        apply_cutoff: bool,
    ) -> PickPackageOptions<'options> {
        PickPackageOptions {
            registry,
            preferred_version_selectors: None,
            pick_lowest_version: false,
            include_latest_tag: false,
            blocked_versions: None,
            policy: pnpm_resolving_npm_resolver::PackagePickPolicy {
                published_by: self.policy.published_by.filter(|_| apply_cutoff),
                published_by_exclude: if apply_cutoff {
                    self.policy.published_by_exclude.as_ref()
                } else {
                    None
                },
                trust_policy: Some(self.config.trust_policy),
            },
            request: pnpm_resolving_npm_resolver::MetadataPickRequest {
                dry_run,
                optional: false,
                update_checksums: false,
            },
        }
    }

    async fn pick_latest(
        &self,
        package_name: &str,
        dry_run: bool,
        registry: &str,
    ) -> Result<Arc<PackageVersion>, ResolveLatestError> {
        let spec = RegistryPackageSpec::latest_tag(package_name);

        let opts = self.pick_options(registry, dry_run, true);
        let ctx = pick_package_context(
            self.http_client,
            self.config,
            &self.policy,
            &self.meta_cache,
            &self.fetch_locker,
        );

        let mut rejected: HashSet<String> = HashSet::new();
        let mut newest_rejected: Option<Arc<PackageVersion>> = None;
        loop {
            let opts = PickPackageOptions {
                blocked_versions: (!rejected.is_empty()).then_some(&rejected),
                ..opts
            };
            let pick = pick_package(&ctx, &spec, &opts).await
                .map_err(|error| ResolveLatestError::Pick(Box::new(error)))?;
            let Some(candidate) = pick.picked_package else {
                // The walk rejected everything the range admits. Hand back
                // the newest of them so the install can name the pin that is
                // too young; claiming the package has no `latest` at all
                // would describe a packument that does not exist. An empty
                // first pick is the real "no latest version".
                return newest_rejected.ok_or_else(|| no_latest_error(package_name, &pick.meta));
            };
            // Every candidate is judged, including the one the bound stops on:
            // short-circuiting on the count would hand back a candidate nobody
            // looked at, and an installable version further down would never be
            // reached. The bound only decides whether to keep walking.
            if self.pins_only_installable_versions(&candidate, dry_run).await? {
                return Ok(candidate);
            }
            if rejected.len() >= GUARD_REPICK_LIMIT {
                // Out of budget with nothing installable found. Hand back the
                // newest candidate and let the install name the pin that is
                // too young, which is a better answer than an error from here.
                return Ok(newest_rejected.unwrap_or(candidate));
            }
            // Reject by the *packument key*, which the next pick filters on.
            // It usually equals the parsed manifest version, but a registry
            // that serves a key differing from the manifest's `version` field
            // would otherwise never get the candidate excluded, and the walk
            // would re-select it forever.
            newest_rejected.get_or_insert_with(|| Arc::clone(&candidate));
            let key = blocked_packument_key(&pick.meta, &candidate, &candidate.version.to_string());
            if !rejected.insert(key) {
                // The picker re-selected a key already rejected, so it cannot
                // be excluded. Hand it back and let the install report the
                // failure rather than spin here.
                return Ok(candidate);
            }
        }
    }

    /// [`Self::pins_only_installable_versions`] for a package named by
    /// `name@version` in the selected registry rather than by an already-picked manifest.
    ///
    /// Errors when that exact version cannot be read back from the registry;
    /// the caller decides what an unreadable candidate means.
    pub(crate) async fn pins_installable_for(
        &self,
        name: &str,
        version: &str,
        registry: &str,
        dry_run: bool,
    ) -> Result<bool, ResolveLatestError> {
        if self.policy.published_by.is_none() {
            return Ok(true);
        }
        let spec = RegistryPackageSpec {
            name: name.to_string(),
            fetch_spec: version.to_string(),
            spec_type: RegistryPackageSpecType::Version,
            revision: None,
            normalized_bare_specifier: None,
        };
        let opts = self.pick_options(registry, dry_run, false);
        let ctx = pick_package_context(
            self.http_client,
            self.config,
            &self.policy,
            &self.meta_cache,
            &self.fetch_locker,
        );
        let pick = pick_package(&ctx, &spec, &opts).await
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
        if self.policy.published_by.is_none() {
            return Ok(true);
        }
        let mut checks = stream::iter(exact_pins(candidate, &self.registry_names))
            .map(|pin| self.pin_is_installable(pin, dry_run))
            .buffer_unordered(self.config.network_concurrency.max(1));
        while let Some(installable) = checks.next().await {
            if !installable {
                return Ok(false);
            }
        }
        Ok(true)
    }

    async fn pin_is_installable(
        &self,
        (spec, registry_name): (RegistryPackageSpec, Option<String>),
        dry_run: bool,
    ) -> bool {
        let Some(cutoff) = self.policy.published_by else { return true };
        if pin_is_exempt(self.policy.published_by_exclude.as_ref(), &spec.name, &spec.fetch_spec) {
            return true;
        }
        let registry = registry_name.map_or_else(
            || pick_registry_for_package(&self.registries, &spec.name, None),
            |name| self.registries_by_prefix[&name].clone(),
        );
        let opts = self.pick_options(&registry, dry_run, true);
        let ctx = pick_package_context(
            self.http_client,
            self.config,
            &self.policy,
            &self.meta_cache,
            &self.fetch_locker,
        );
        let Ok(pick) = pick_package(&ctx, &spec, &opts).await else { return true };
        pick.meta
            .published_at(&spec.fetch_spec)
            .and_then(parse_packument_timestamp)
            .is_none_or(|published| published <= cutoff)
    }
}

/// Whether `minimumReleaseAgeExclude` lets `name@pinned` install at any age.
///
/// A version-qualified exclusion exempts the versions it names and no
/// others, so an immature pin on a different version of the same package
/// still counts against the candidate.
fn pin_is_exempt(policy: Option<&PackageVersionPolicy>, name: &str, pinned: &str) -> bool {
    match policy.map(|policy| policy.matches(name)) {
        Some(PolicyMatch::AnyVersion) => true,
        Some(PolicyMatch::ExactVersions(versions)) => versions
            .iter()
            .any(|exact| exact == pinned),
        _ => false,
    }
}

/// Exact registry specifiers a manifest requires, normalized by the resolver's
/// parser and paired with their named-registry routing when applicable.
///
/// `optionalDependencies` is included because a lockfile records every
/// platform's binary, so an immature one blocks the install on every
/// platform, not just its own. A name declared in both groups resolves to
/// its optional declaration, which is therefore the only one judged —
/// otherwise a specifier the install never uses could pass the candidate
/// over.
fn exact_pins<'a>(
    candidate: &'a PackageVersion,
    registry_names: &'a HashSet<String>,
) -> impl Iterator<Item = (RegistryPackageSpec, Option<String>)> + 'a {
    let optional = candidate.optional_dependencies.iter().flatten();
    let required = candidate.dependencies
        .iter()
        .flatten()
        .filter(|(name, _)| {
            !candidate.optional_dependencies
                .as_ref()
                .is_some_and(|optional| optional.contains_key(*name))
        });
    optional
        .chain(required)
        .filter_map(|(name, raw)| {
            let (spec, registry) = match parse_named_registry_specifier_to_registry_package_spec(
                raw,
                registry_names,
                Some(name),
                "latest",
            )
            .ok()?
            {
                Some(named) => (named.spec, Some(named.registry_name)),
                None => (parse_bare_specifier(raw, Some(name), "latest", "")?, None),
            };
            (spec.spec_type == RegistryPackageSpecType::Version).then_some((spec, registry))
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
    fn exhaustion_policy(&self) -> pnpm_resolving_resolver_base::GuardExhaustionPolicy {
        pnpm_resolving_resolver_base::GuardExhaustionPolicy::AcceptRejected
    }

    fn check<'a>(&'a self, name: &'a str, version: &'a str) -> PackageVersionGuardFuture<'a> {
        Box::pin(async move {
            let registries = self.config
                .resolved_registries()
                .into_iter()
                .collect();
            let registry = pick_registry_for_package(&registries, name, None);
            self.check_in_registry(name, version, &registry).await
        })
    }

    fn check_in_registry<'a>(
        &'a self,
        name: &'a str,
        version: &'a str,
        registry: &'a str,
    ) -> PackageVersionGuardFuture<'a> {
        Box::pin(async move {
            let picker = LatestPicker::new(
                &self.config,
                &self.http_client,
                self.policy.clone(),
                Arc::clone(&self.meta_cache),
                Arc::clone(&self.fetch_locker),
            );
            Ok(match picker.pins_installable_for(name, version, registry, self.dry_run).await {
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

fn no_latest_error(name: &str, meta: &pnpm_registry::Package) -> ResolveLatestError {
    let Some((version, error)) = meta.latest_decode_error() else {
        return ResolveLatestError::NoLatestVersion;
    };
    ResolveLatestError::UndecodableLatestManifest {
        name: name.to_string(),
        version: redact_and_sanitize(version),
        error: redact_and_sanitize(&error),
    }
}
