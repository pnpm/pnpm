//! Dispatcher-side surface of `@pnpm/resolving.resolver-base`. Defines
//! the `WantedDependency` → `ResolveResult` contract and the
//! [`Resolver`] trait every per-protocol resolver implements.
//!
//! Future per-protocol resolvers (npm, git, tarball, local, jsr,
//! runtimes, named-registry, workspace) implement [`Resolver`]; the
//! default-resolver dispatcher composes them into a chain mirroring
//! pnpm's
//! [`createResolver`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/default-resolver/src/index.ts#L97-L173).

use std::{collections::BTreeMap, future::Future, path::PathBuf, pin::Pin, sync::Arc};

use chrono::{DateTime, Utc};
use derive_more::{Display, From};
use pacquet_config::{TrustPolicy, version_policy::PackageVersionPolicy};
use pacquet_lockfile::{LockfileResolution, PkgNameVer};
use serde::{Deserialize, Serialize};

use crate::verifier::ResolutionPolicyViolation;

/// Branded resolution identifier the resolver chain emits on every
/// successful pick. Mirrors pnpm's
/// [`PkgResolutionId`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/core/types/src/misc.ts#L59)
/// — a phantom-typed string with no runtime validator.
///
/// Two shapes appear in the wild:
/// * `name@version` from the npm-registry resolver.
/// * URL-shaped (`git+https://…#sha`, `https://codeload.github.com/…/tar.gz/sha`,
///   `file:…`) from the git / local / tarball resolvers.
///
/// Consumers that need the structured `name@version` form read
/// [`ResolveResult::name_ver`] instead.
#[derive(Debug, Display, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, From)]
#[serde(transparent)]
pub struct PkgResolutionId(String);

impl PkgResolutionId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl From<&str> for PkgResolutionId {
    fn from(value: &str) -> Self {
        PkgResolutionId(value.to_string())
    }
}

impl From<&PkgNameVer> for PkgResolutionId {
    fn from(value: &PkgNameVer) -> Self {
        PkgResolutionId(value.to_string())
    }
}

impl From<PkgNameVer> for PkgResolutionId {
    fn from(value: PkgNameVer) -> Self {
        PkgResolutionId(value.to_string())
    }
}

/// An entry from a project's manifest that the resolver chain will
/// route to a concrete protocol. Mirrors pnpm's
/// [`WantedDependency`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L304-L313).
///
/// At least one of `alias` and `bare_specifier` is *expected* to be
/// populated. Upstream models this with a discriminated union;
/// pacquet keeps both fields as `Option<String>` for ergonomic field
/// access and uses `#[derive(Default)]` only so call sites can write
/// `..WantedDependency::default()` in struct literals — a bare
/// `WantedDependency::default()` with both halves `None` is a
/// programming error the type system doesn't catch. The invariant is
/// upheld by construction sites (the parse-wanted-dependency port
/// and the deps-resolver's manifest reader); resolvers that walk a
/// `WantedDependency` with both halves empty should return
/// `Ok(None)` so the chain falls through to the
/// "spec not supported" terminal.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WantedDependency {
    /// Local install name in `node_modules/`. For `foo@1.2.3` this is
    /// `Some("foo")`; for the npm-alias form `foo@npm:lodash@^4` it
    /// is also `Some("foo")`.
    pub alias: Option<String>,
    /// Protocol-prefixed selector the resolver chain dispatches on.
    /// For `foo@1.2.3` this is `Some("1.2.3")`; for `git+ssh://…` it
    /// is the whole input.
    pub bare_specifier: Option<String>,
    /// Whether the dep is being installed as injected (workspace
    /// package copied into the importer's `node_modules/` rather than
    /// linked).
    pub injected: Option<bool>,
    /// Pre-existing specifier from the lockfile, supplied so resolvers
    /// can prefer the previously-pinned version when no update is
    /// requested.
    pub prev_specifier: Option<String>,
    /// `true` when the entry came from `optionalDependencies`.
    /// Resolvers may downgrade failures to warnings for optional deps.
    pub optional: Option<bool>,
}

/// Allocation-friendly map type for [`PreferredVersions`].
///
/// `BTreeMap` (not `HashMap`) keeps iteration order stable across
/// runs, which matters because the deps-resolver consults these to
/// break version ties — a flapping order would let identical inputs
/// produce different lockfile picks.
pub type PreferredVersions = BTreeMap<String, VersionSelectors>;

/// Per-package set of selectors and their weights. Mirrors pnpm's
/// [`VersionSelectors`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L264-L266).
pub type VersionSelectors = BTreeMap<String, VersionSelectorEntry>;

/// Discriminator for how a selector should be interpreted. Mirrors
/// pnpm's
/// [`VersionSelectorType`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L262).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VersionSelectorType {
    Version,
    Range,
    Tag,
}

/// One selector with a tie-break weight. Mirrors pnpm's
/// [`VersionSelectorWithWeight`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L268-L271).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionSelectorWithWeight {
    pub selector_type: VersionSelectorType,
    pub weight: u32,
}

/// A [`VersionSelectors`] map value: upstream stores either a plain
/// [`VersionSelectorType`] or a [`VersionSelectorWithWeight`]. Mirrors
/// pnpm's
/// [`VersionSelectorWithWeight | VersionSelectorType`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L265)
/// union.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionSelectorEntry {
    Plain(VersionSelectorType),
    Weighted(VersionSelectorWithWeight),
}

/// Selector weight applied to direct dependencies. Mirrors pnpm's
/// [`DIRECT_DEP_SELECTOR_WEIGHT`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L250).
pub const DIRECT_DEP_SELECTOR_WEIGHT: u32 = 1_000;

/// Selector weight applied to versions already pinned in the wanted
/// lockfile. Must outrank [`DIRECT_DEP_SELECTOR_WEIGHT`] so that
/// existing pins stick across an add of a fresh range. Mirrors pnpm's
/// [`EXISTING_VERSION_SELECTOR_WEIGHT`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L260).
pub const EXISTING_VERSION_SELECTOR_WEIGHT: u32 = 1_000_000;

/// One project in the current workspace that resolution can satisfy
/// `workspace:`-protocol entries from. Mirrors pnpm's
/// [`WorkspacePackage`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L239-L242).
///
/// `manifest` is held as an opaque [`DependencyManifest`] alias today
/// (a thin wrapper around `serde_json::Value`); once `package-manifest`
/// gains a typed in-memory manifest, swap the alias.
#[derive(Debug, Clone)]
pub struct WorkspacePackage {
    pub root_dir: PathBuf,
    pub manifest: DependencyManifest,
}

/// Workspace packages indexed by version string. Mirrors pnpm's
/// [`WorkspacePackagesByVersion`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L244).
pub type WorkspacePackagesByVersion = BTreeMap<String, WorkspacePackage>;

/// Workspace packages indexed by name, then by version. Mirrors pnpm's
/// [`WorkspacePackages`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L246).
pub type WorkspacePackages = BTreeMap<String, WorkspacePackagesByVersion>;

/// Reload behavior the dispatcher passes per-resolve. Mirrors pnpm's
/// [`ResolveOptions.update`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L291)
/// tri-state (`false | 'compatible' | 'latest'`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum UpdateBehavior {
    /// Keep the lockfile-pinned version. Equivalent to upstream's `false`.
    #[default]
    Off,
    /// Bump within the current range, mirroring upstream's `'compatible'`.
    Compatible,
    /// Bump to the latest, mirroring upstream's `'latest'`.
    Latest,
}

/// Options the dispatcher hands a resolver per-resolve. Mirrors pnpm's
/// [`ResolveOptions`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L277-L302).
#[derive(Debug, Default, Clone)]
pub struct ResolveOptions {
    pub project_dir: PathBuf,
    pub lockfile_dir: PathBuf,
    /// Lockfile + manifest preferred-versions seed the npm picker biases
    /// toward (so pins that still satisfy their range survive a
    /// re-resolve). Held behind [`Arc`] because the tree walker clones
    /// `ResolveOptions` per depth tier and the install layer clones it
    /// per importer — sharing the (potentially large) map keeps those
    /// clones to a refcount bump.
    pub preferred_versions: Arc<PreferredVersions>,
    pub workspace_packages: Option<WorkspacePackages>,
    pub default_tag: Option<String>,
    pub pick_lowest_version: bool,
    pub prefer_workspace_packages: bool,
    pub always_try_workspace_packages: bool,
    pub update: UpdateBehavior,
    /// When `true`, bypass cached metadata fast paths so the registry
    /// is the authority on integrity values. Mirrors pnpm's
    /// `--update-checksums`.
    pub update_checksums: bool,
    pub inject_workspace_packages: bool,
    pub calc_specifier: bool,
    /// `minimumReleaseAge` cutoff. Versions published after this point
    /// are filtered out by the npm picker (or reported inline via
    /// [`ResolveResult::policy_violation`] when no mature pick exists).
    /// `None` disables the maturity filter.
    pub published_by: Option<DateTime<Utc>>,
    /// Per-package exclude policy for the maturity filter. `None`
    /// applies the filter uniformly.
    pub published_by_exclude: Option<PackageVersionPolicy>,
    /// `trustPolicy='no-downgrade'` gate. When `Some(NoDowngrade)`, the
    /// npm resolver rejects a freshly picked version whose trust
    /// evidence is weaker than an earlier-published version's — the
    /// resolver-time counterpart to the lockfile verifier's check.
    /// `None`/`Some(Off)` disables it. Mirrors pnpm's resolver-time
    /// [`failIfTrustDowngraded`](https://github.com/pnpm/pnpm/blob/372cae6a55/resolving/npm-resolver/src/index.ts#L548-L550)
    /// call, gated on `opts.trustPolicy === 'no-downgrade'`.
    pub trust_policy: Option<TrustPolicy>,
    /// Per-package exclude policy for the trust gate. `None` applies
    /// the gate uniformly.
    pub trust_policy_exclude: Option<PackageVersionPolicy>,
    /// Max age, in minutes, before which the trust gate still applies.
    /// A picked version older than this skips the check. `None` always
    /// checks. Mirrors pnpm's `trustPolicyIgnoreAfter`.
    pub trust_policy_ignore_after: Option<u64>,
    /// `true` suppresses on-disk and in-memory cache write-back during
    /// resolution. Mirrors upstream's `dryRun` flag at the resolver
    /// boundary.
    pub dry_run: bool,
    /// When `true`, reject exotic (git, tarball, file, ...) dependencies
    /// appearing anywhere below the importer. Direct dependencies are
    /// still allowed; only transitive deps are gated. The check
    /// consults [`ResolveResult::resolved_via`] against the closed set
    /// of non-exotic provenance tags. Mirrors pnpm's
    /// [`blockExoticSubdeps`](https://github.com/pnpm/pnpm/blob/df990fdb51/installing/deps-resolver/src/resolveDependencies.ts#L1420-L1434).
    pub block_exotic_subdeps: bool,
}

/// In-memory manifest shape a resolver may attach to its
/// [`ResolveResult`]. Mirrors pnpm's
/// [`DependencyManifest`](https://github.com/pnpm/pnpm/blob/3687b0e180/packages/types/src/index.ts)
/// (sourced from `@pnpm/types` upstream).
///
/// Today this aliases [`serde_json::Value`] so the seam compiles
/// without a typed manifest port. The `package-manifest` crate's
/// `PackageManifest` is a file-handle wrapper, not the value type
/// upstream's [`DependencyManifest`] denotes; once the typed
/// in-memory manifest lands, swap this alias for it.
pub type DependencyManifest = serde_json::Value;

/// `Arc`-shared variant of [`DependencyManifest`], used in
/// [`ResolveResult::manifest`]. Wrapping the manifest avoids the
/// deep-clone of the JSON tree every time a `ResolveResult`
/// propagates — the deps-resolver stores one copy in
/// `ResolvedPackage` and another in each `DependenciesGraph` node,
/// each `Clone` cost dropped from O(manifest size) to a refcount
/// bump. Mirrors JS object-reference semantics — pnpm's
/// `resolveResult.manifest` is an object, not a deep copy.
pub type SharedDependencyManifest = Arc<DependencyManifest>;

/// Outcome of one [`Resolver::resolve`] call when the resolver claims
/// the wanted dependency. Mirrors pnpm's
/// [`ResolveResult`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L212-L237).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolveResult {
    /// Branded resolution identifier — see [`PkgResolutionId`].
    pub id: PkgResolutionId,
    /// Structured `name@version` when the resolver knows both at
    /// resolve time. The npm-registry resolver always fills this;
    /// resolvers that learn the package name from the manifest only
    /// after the fetch (git / tarball / local) leave it `None` and
    /// downstream consumers (virtual-store layout, dedupe keys) must
    /// fall back to reading the manifest. Mirrors the upstream
    /// pattern where `result.manifest.name` and `result.manifest.version`
    /// are the canonical name/version sources for non-npm resolutions.
    pub name_ver: Option<PkgNameVer>,
    /// `latest` tag at the moment of resolution. Filled by the npm
    /// resolver; absent for protocols that have no notion of latest
    /// (git, file, link, ...).
    pub latest: Option<String>,
    /// ISO-8601 publish timestamp. Filled by the npm resolver when
    /// available; consulted by the `minimumReleaseAge` verifier.
    pub published_at: Option<String>,
    /// The manifest fragment the resolver fetched. Optional because
    /// some protocols defer manifest reading to the fetch step.
    /// Held as [`SharedDependencyManifest`] (`Arc`-shared) so the
    /// deps-resolver's tree walk and the per-snapshot graph copies
    /// don't deep-clone the JSON tree per occurrence.
    pub manifest: Option<SharedDependencyManifest>,
    /// Where the artifact lives. Pacquet reuses
    /// [`LockfileResolution`] for this — same shape as upstream's
    /// `Resolution`, which is the discriminated union over
    /// tarball/registry/directory/git/binary/variations.
    pub resolution: LockfileResolution,
    /// Provenance tag (`"npm-registry"`, `"git-repository"`,
    /// `"local-tarball"`, ...). Used by deps-installer logs and by
    /// `@pnpm/cli.default-reporter`.
    pub resolved_via: String,
    /// Resolver's normalized echo of the bare specifier (e.g. `"^4"`
    /// for an npm range). Used to update the manifest's recorded
    /// spec when `add` or `update` runs.
    pub normalized_bare_specifier: Option<String>,
    /// Alias from the wanted dependency. Threaded through so the
    /// install layer can address the resolved package by its local
    /// name. See upstream's
    /// [`alias` field](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L220).
    pub alias: Option<String>,
    /// Set when the resolver picked this version despite a policy
    /// violation (e.g. immature relative to `publishedBy`, trust
    /// downgrade detected by `failIfTrustDowngraded`). Mirrors
    /// upstream's
    /// [`policyViolation`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L221-L236)
    /// field; the deps-resolver aggregates these across every resolve
    /// call into a single set the install command can react to.
    pub policy_violation: Option<ResolutionPolicyViolation>,
}

/// Input to [`Resolver::resolve_latest`]. The resolver decides whether
/// it owns this dep purely from `wanted_dependency` — the lockfile-
/// resolved ref is the caller's concern, not the resolver's. Mirrors
/// pnpm's
/// [`LatestQuery`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L323-L326).
#[derive(Debug, Clone)]
pub struct LatestQuery {
    pub wanted_dependency: WantedDependency,
    pub compatible: bool,
}

/// Result of [`Resolver::resolve_latest`]. Mirrors pnpm's
/// [`LatestInfo`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L339-L341).
///
/// The dispatcher distinguishes "this resolver does not handle this dep"
/// (`Ok(None)`) from "I claim it but can't say what's latest"
/// (`Ok(Some(LatestInfo { latest_manifest: None }))`).
#[derive(Debug, Default, Clone)]
pub struct LatestInfo {
    pub latest_manifest: Option<SharedDependencyManifest>,
}

/// Error type the resolver seam uses. Boxed-trait-object today so each
/// resolver crate can keep its own typed error enum without forcing a
/// shared enum prematurely. Once enough resolvers are ported to make
/// the common error shape clear, tighten this to a concrete enum.
pub type ResolveError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Boxed-future return type for [`Resolver::resolve`]. Same
/// `dyn Trait` ergonomics rationale as [`crate::VerifyFuture`].
pub type ResolveFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Option<ResolveResult>, ResolveError>> + Send + 'a>>;

/// Boxed-future return type for [`Resolver::resolve_latest`].
pub type ResolveLatestFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Option<LatestInfo>, ResolveError>> + Send + 'a>>;

/// One per-protocol resolver. Mirrors the per-resolver shape upstream
/// composes into the chain at
/// [`createResolver`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/default-resolver/src/index.ts#L97-L173):
/// each returns `Ok(None)` to defer to the next resolver in the chain
/// and `Ok(Some(_))` to claim the wanted dependency.
///
/// `resolve_latest` is the companion `pnpm outdated` / `pnpm update --latest`
/// path uses; resolvers that have no notion of "latest" (file, link,
/// workspace) return `Ok(Some(LatestInfo { latest_manifest: None }))`
/// when they claim the wanted dep and `Ok(None)` otherwise.
pub trait Resolver: Send + Sync {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a>;

    fn resolve_latest<'a>(
        &'a self,
        query: &'a LatestQuery,
        opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a>;
}
