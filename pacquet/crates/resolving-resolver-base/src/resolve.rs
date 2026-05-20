//! Dispatcher-side surface of `@pnpm/resolving.resolver-base`. Defines
//! the `WantedDependency` → `ResolveResult` contract and the
//! [`Resolver`] trait every per-protocol resolver implements.
//!
//! Future per-protocol resolvers (npm, git, tarball, local, jsr,
//! runtimes, named-registry, workspace) implement [`Resolver`]; the
//! default-resolver dispatcher composes them into a chain mirroring
//! pnpm's
//! [`createResolver`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/default-resolver/src/index.ts#L97-L173).

use std::{collections::BTreeMap, future::Future, path::PathBuf, pin::Pin};

use pacquet_lockfile::{LockfileResolution, PkgNameVer};
use serde::{Deserialize, Serialize};

use crate::verifier::ResolutionPolicyViolation;

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
///
/// Trust / published-at fields are not modeled yet — they belong to
/// the npm resolver's verifier surface, which already lives at
/// `resolving-npm-resolver`. They'll be added here when the
/// dispatcher's npm leg actually needs to pass them through.
#[derive(Debug, Default, Clone)]
pub struct ResolveOptions {
    pub project_dir: PathBuf,
    pub lockfile_dir: PathBuf,
    pub preferred_versions: PreferredVersions,
    pub workspace_packages: Option<WorkspacePackages>,
    pub default_tag: Option<String>,
    pub pick_lowest_version: bool,
    pub prefer_workspace_packages: bool,
    pub always_try_workspace_packages: bool,
    pub update: UpdateBehavior,
    pub inject_workspace_packages: bool,
    pub calc_specifier: bool,
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

/// Outcome of one [`Resolver::resolve`] call when the resolver claims
/// the wanted dependency. Mirrors pnpm's
/// [`ResolveResult`](https://github.com/pnpm/pnpm/blob/3687b0e180/resolving/resolver-base/src/index.ts#L212-L237).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolveResult {
    /// Branded `{name}@{version}` identifier upstream calls
    /// `PkgResolutionId`. Pacquet reuses
    /// [`pacquet_lockfile::PkgNameVer`], which already pins the same
    /// shape used elsewhere in the codebase.
    pub id: PkgNameVer,
    /// `latest` tag at the moment of resolution. Filled by the npm
    /// resolver; absent for protocols that have no notion of latest
    /// (git, file, link, …).
    pub latest: Option<String>,
    /// ISO-8601 publish timestamp. Filled by the npm resolver when
    /// available; consulted by the `minimumReleaseAge` verifier.
    pub published_at: Option<String>,
    /// The manifest fragment the resolver fetched. Optional because
    /// some protocols defer manifest reading to the fetch step.
    pub manifest: Option<DependencyManifest>,
    /// Where the artifact lives. Pacquet reuses
    /// [`LockfileResolution`] for this — same shape as upstream's
    /// `Resolution`, which is the discriminated union over
    /// tarball/registry/directory/git/binary/variations.
    pub resolution: LockfileResolution,
    /// Provenance tag (`"npm-registry"`, `"git-repository"`,
    /// `"local-tarball"`, …). Used by deps-installer logs and by
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
    pub latest_manifest: Option<DependencyManifest>,
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
