use super::{LogLevel, Serialize};

/// `pnpm:package-manifest` payload. The bunyan-envelope `level` is a
/// fixed outer field; the rest is a presence-tagged union — pnpm
/// keys on whether `initial` or `updated` is present rather than
/// using a `status` discriminator. `#[serde(untagged)]` matches
/// that shape; `#[serde(flatten)]` keeps `prefix` adjacent to
/// `initial` / `updated` at the top level.
#[derive(Debug, Clone, Serialize)]
pub struct PackageManifestLog {
    pub level: LogLevel,
    #[serde(flatten)]
    pub message: PackageManifestMessage,
}

/// `pnpm:package-manifest` discriminated payload. The `Value` carries
/// the entire on-disk `package.json` body — pnpm's reporter doesn't
/// pick fields out, it threads the manifest through to consumers
/// like the audit pipeline that need the full thing.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum PackageManifestMessage {
    Initial { prefix: String, initial: serde_json::Value },
    Updated { prefix: String, updated: serde_json::Value },
}

/// `pnpm:root` payload. Same flatten-on-presence pattern as
/// [`PackageManifestLog`].
#[derive(Debug, Clone, Serialize)]
pub struct RootLog {
    pub level: LogLevel,
    #[serde(flatten)]
    pub message: RootMessage,
}

/// `pnpm:root` discriminated payload. pnpm's reporter dispatches on
/// whether `added` or `removed` is present; tag-on-presence matches
/// that. Pacquet only emits `added` today (no pruning pipeline yet)
/// — `Removed` is here to pin the wire shape so the channel is
/// usable when pruning lands.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum RootMessage {
    Added { prefix: String, added: AddedRoot },
    Removed { prefix: String, removed: RemovedRoot },
}

/// `added` payload on a [`RootMessage::Added`] event. `name` is the
/// directory name under `node_modules/` (the manifest alias for
/// npm-aliased entries; the package name otherwise). `real_name`
/// is the registry name. The other fields are optional in pnpm's
/// shape; pacquet populates what it has from the lockfile snapshot
/// today.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedRoot {
    pub name: String,
    pub real_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency_type: Option<DependencyType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_from: Option<String>,
}

/// `removed` payload on a [`RootMessage::Removed`] event. Optional
/// fields match pnpm's shape and are skipped when absent.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedRoot {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependency_type: Option<DependencyType>,
}

/// Direct-dependency category. Mirrors pnpm's three-value union;
/// peer dependencies are not a separate emit and don't appear here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DependencyType {
    Prod,
    Dev,
    Optional,
}

/// `pnpm:skipped-optional-dependency` payload.
///
/// The wire shape is a discriminated union over `reason` with two
/// distinct `package` shapes: `build_failure` / `unsupported_engine`
/// / `unsupported_platform` all carry `package: { id, name, version }`;
/// `resolution_failure` carries `package: { name?, version?,
/// bareSpecifier }` with no `id`.
///
/// The `reason` and `package` shapes co-vary. The `package` field
/// below is therefore a `#[serde(untagged)]` enum that picks the
/// right shape depending on which variant the emit site constructs.
/// The pairing is not type-enforced against `reason` (a
/// `BuildFailure` reason with a `ResolutionFailure` package is
/// constructible in Rust); emit sites live in
/// `pnpm-package-manager` (`installability.rs` for the
/// installability skips, `build_modules.rs` for the build-failure
/// path) and must keep the pairing correct by hand.
/// `CreateVirtualStore`'s slice 4 fetch-failure path is silent on
/// the reporter wire — it only swallows the error, no event is
/// emitted from there — so it isn't a constructor site for this
/// log. Tightening the pairing into a closed-set builder API
/// would constrain a future resolver port without adding much
/// real safety, so it's left to convention until a site actually
/// pairs the wrong shapes.
///
/// `parents` co-varies with `reason` the same way `package` does:
/// only the resolver-side `resolution_failure` emit carries it
/// (empty for a direct optional dependency of the importer); every
/// other emit site omits it, matching pnpm's payloads.
#[derive(Debug, Clone, Serialize)]
pub struct SkippedOptionalDependencyLog {
    pub level: LogLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    pub package: SkippedOptionalPackage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parents: Option<Vec<SkippedOptionalParent>>,
    pub prefix: String,
    pub reason: SkippedOptionalReason,
}

/// One ancestor on a `resolution_failure` skip's `parents` chain: a
/// resolved package between the importer and the failing optional
/// edge. The default reporter renders only skips whose chain is empty
/// (a direct optional dependency), matching pnpm's
/// `reportSkippedOptionalDependencies`.
#[derive(Debug, Clone, Serialize)]
pub struct SkippedOptionalParent {
    pub id: String,
    pub name: String,
    pub version: String,
}

/// Package identifier carried on a [`SkippedOptionalDependencyLog`].
/// Two shapes, depending on `reason`:
///
/// - [`SkippedOptionalPackage::Installed`] — `{ id, name, version }`
///   for `build_failure` / `unsupported_engine` /
///   `unsupported_platform`. Used by the slice 1 emit site in
///   `installability.rs` and the build-failure emit in
///   `build_modules.rs`.
/// - [`SkippedOptionalPackage::ResolutionFailure`] —
///   `{ name?, version?, bareSpecifier }` for `resolution_failure`.
///   Emitted by the deps resolver's skipped-optional sink when an
///   optional dependency's resolution failure drops the edge.
///
/// `#[serde(untagged)]` so each variant serializes as its own object
/// shape — a union of two `package: { ... }` types.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum SkippedOptionalPackage {
    /// `{ id, name, version }` shape used by every non-resolver
    /// emit (installability + build-failure).
    Installed { id: String, name: String, version: String },
    /// `{ name?, version?, bareSpecifier }` shape used by the
    /// resolver-side `resolution_failure` emit (the deps resolver's
    /// skipped-optional sink wired in
    /// `install_with_fresh_lockfile.rs`). `name` and `version` are
    /// optional and stay `None` when the resolver fails before it
    /// could resolve those fields.
    ResolutionFailure {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        #[serde(rename = "bareSpecifier")]
        bare_specifier: String,
    },
}

/// Discriminator on a [`SkippedOptionalDependencyLog`]. See
/// [`SkippedOptionalPackage`] for which emit site pairs with which
/// reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkippedOptionalReason {
    BuildFailure,
    UnsupportedEngine,
    UnsupportedPlatform,
    ResolutionFailure,
}
