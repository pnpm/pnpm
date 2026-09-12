use super::{
    Arc, DateTime, Hash, Hasher, PathBuf, PkgNameVerPeer, PkgResolutionId, ResolveOptions, Utc,
    WorkspacePackages,
};

/// Cache key for [`WorkspaceTreeCtx`](crate::WorkspaceTreeCtx)'s `resolved_by_wanted` map.
///
/// The npm-shaped slice pacquet exposes today calls
/// [`Resolver::resolve`] with four [`WantedDependency`] fields
/// populated — `alias`, `bare_specifier`, `optional`, and `injected` (see
/// the `WantedDependency` literals in [`extend_tree`] and the recursive
/// arm of [`fn@resolve_node`]). Anything else stays at `Default::default()`,
/// so a tuple over those four fields uniquely identifies a wanted
/// dep across revisits.
///
/// `optional` is part of the key because the npm resolver's
/// `pick_package` toggles between the abbreviated and full packument
/// based on `wanted.optional` — caching by `(alias, bare_specifier)`
/// alone would let an optional caller satisfy itself with a
/// non-optional caller's abbreviated result, losing the
/// `libc`/`cpu`/`os` filter inputs that mode supplies.
///
/// `injected` is part of the key because the workspace branch of the
/// npm resolver returns a `file:<path>` resolution when the dep is
/// injected and a `link:<path>` resolution otherwise (see
/// `resolve_from_local_package`). Two importers asking for the same
/// workspace dep with different `dependenciesMeta[*].injected` flags
/// must take different cache slots.
///
/// `pick_lowest_version` and `published_by` are part of the key because
/// `resolutionMode` makes the version pick depend on them: under
/// `time-based` / `lowest-direct` a direct dependency is resolved
/// lowest while a transitive one is resolved highest, and under
/// `time-based` transitive deps carry a publish-date cutoff a direct
/// dep does not. The same wanted spec (`react@^18`) can therefore
/// resolve to a different version as a direct vs. transitive dep, so
/// the two occurrences must take different cache slots. In `highest`
/// mode (the default) every occurrence shares the same pair, so the
/// dedup is unchanged.
///
/// `project_dir` is part of the key for any specifier that can produce
/// a project-relative resolution. This includes explicit local
/// specifiers (`link:` / `file:` / `workspace:`) and normal semver
/// specifiers in workspace mode, because `linkWorkspacePackages` can
/// replace the registry pick with a workspace package. A non-injected
/// workspace dep resolves through `resolve_from_local_package` to a
/// `link:<path>` whose `<path>` is computed *relative to the
/// consuming importer's directory*. Without `project_dir` in the key,
/// the first importer to resolve `(@scope/lib, ^1.0.0)` would
/// seed the workspace-wide cache with its own relative path and every
/// other importer would reuse it verbatim — e.g. a root resolving to
/// `link:packages/lib` would hand `packages/app` the same string,
/// which from `packages/app` points at the non-existent
/// `packages/app/packages/lib`.
///
/// The final two fields isolate importers with active update policies
/// and record whether this wanted dependency is an explicit update target.
/// Ordinary keep-all importers use no importer scope and retain the existing
/// cross-importer cache sharing.
///
/// [`Resolver::resolve`]: pnpm_resolving_resolver_base::Resolver::resolve
/// [`WantedDependency`]: pnpm_resolving_resolver_base::WantedDependency
/// [`extend_tree`]: super::super::extend_tree
/// [`fn@resolve_node`]: super::super::walk::resolve_node
pub(in super::super) type WantedKeyFields = (
    Option<String>,
    Option<String>,
    Option<bool>,
    Option<bool>,
    bool,
    Option<DateTime<Utc>>,
    Option<PathKey>,
    Option<PkgNameVerPeer>,
    Vec<(String, Vec<String>)>,
    Option<String>,
    bool,
);

/// A [`WantedKeyFields`] tuple with its hashes fixed at construction —
/// the full one, and a consumer-scope-less one for
/// [`SharedWorkspaceWantedKey`] — so an edge's key is hashed once
/// however many maps and derived keys carry it. Cloning is cheap.
#[derive(Debug, Clone)]
pub(in super::super) struct WantedKey(Arc<WantedKeyInner>);

#[derive(Debug)]
pub(super) struct WantedKeyInner {
    pub(super) full_hash: u64,
    pub(super) scopeless_hash: u64,
    pub(super) fields: WantedKeyFields,
}

impl WantedKey {
    pub(in super::super) fn new(fields: WantedKeyFields) -> Self {
        // One pass over the fields serves both hashes: everything but
        // the consumer scope feeds a hasher whose intermediate state is
        // snapshotted for the scope-less hash before the scope joins.
        let mut hasher = rustc_hash::FxHasher::default();
        (
            &fields.0, &fields.1, &fields.2, &fields.3, &fields.4, &fields.5, &fields.7, &fields.8,
            &fields.9, &fields.10,
        )
            .hash(&mut hasher);
        let scopeless_hash = hasher.clone().finish();
        fields.6.hash(&mut hasher);
        WantedKey(Arc::new(WantedKeyInner { full_hash: hasher.finish(), scopeless_hash, fields }))
    }

    pub(in super::super) fn fields(&self) -> &WantedKeyFields {
        &self.0.fields
    }

    /// Field-wise equality without the consumer-scope slot — the
    /// equality [`SharedWorkspaceWantedKey`] shares between importers.
    pub(super) fn scopeless_eq(&self, other: &Self) -> bool {
        let left = &self.0.fields;
        let right = &other.0.fields;
        left.0 == right.0
            && left.1 == right.1
            && left.2 == right.2
            && left.3 == right.3
            && left.4 == right.4
            && left.5 == right.5
            && left.7 == right.7
            && left.8 == right.8
            && left.9 == right.9
            && left.10 == right.10
    }
}

impl PartialEq for WantedKey {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
            || (self.0.full_hash == other.0.full_hash && self.0.fields == other.0.fields)
    }
}

impl Eq for WantedKey {}

impl Hash for WantedKey {
    fn hash<State: Hasher>(&self, state: &mut State) {
        state.write_u64(self.0.full_hash);
    }
}

/// A path slot of a resolver cache key.
///
/// `Path`'s own `Hash` and `Eq` walk the path component by component,
/// and the per-edge key lookups made that walk one of the hottest
/// spots of a large workspace's resolution. The paths that reach these
/// keys come from one canonical config-derived source per importer, so
/// this wrapper compares and hashes the underlying `OsStr` — a plain
/// byte comparison. That is *stricter* than component equality
/// (`a//b` ≠ `a/b` here), which for a dedup cache can only cost an
/// extra identical resolution, never conflate two different paths.
#[derive(Debug, Clone, derive_more::From)]
pub(in super::super) struct PathKey(pub(in super::super) PathBuf);

impl PartialEq for PathKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_os_str() == other.0.as_os_str()
    }
}

impl Eq for PathKey {}

impl Hash for PathKey {
    fn hash<State: Hasher>(&self, state: &mut State) {
        self.0.as_os_str().hash(state);
    }
}

/// A wanted dependency key without its consumer directory, plus the resolver
/// inputs that may vary between importers.
///
/// Holds the edge's full [`WantedKey`]; `Hash`/`Eq` drop the consumer
/// directory, so keys built by different importers match by value.
#[derive(Debug, Clone)]
pub(in super::super) struct SharedWorkspaceWantedKey {
    pub(super) wanted: WantedKey,
    pub(super) previous_specifier: Option<String>,
    // Behind an `Arc` because the fields are invariant per importer
    // (see [`WorkspaceResolutionOptionsKey`]) while a key is built per
    // dependency edge; `Hash`/`Eq` see through the `Arc`, so keys
    // built by different importers still match by value.
    pub(super) resolve_options: Arc<WorkspaceResolutionOptionsKey>,
}

impl SharedWorkspaceWantedKey {
    pub(in super::super) fn new(
        wanted: WantedKey,
        previous_specifier: Option<String>,
        resolve_options: &Arc<WorkspaceResolutionOptionsKey>,
    ) -> Self {
        Self { wanted, previous_specifier, resolve_options: Arc::clone(resolve_options) }
    }
}

impl PartialEq for SharedWorkspaceWantedKey {
    fn eq(&self, other: &Self) -> bool {
        self.wanted.scopeless_eq(&other.wanted)
            && self.previous_specifier == other.previous_specifier
            && self.resolve_options == other.resolve_options
    }
}

impl Eq for SharedWorkspaceWantedKey {}

impl Hash for SharedWorkspaceWantedKey {
    fn hash<State: Hasher>(&self, state: &mut State) {
        state.write_u64(self.wanted.0.scopeless_hash);
        self.previous_specifier.hash(state);
        self.resolve_options.hash(state);
    }
}

/// Resolver inputs that can change a named workspace resolution independently
/// of the consuming project directory.
///
/// Every field is invariant across the [`ResolveOptions`] variants one
/// importer's walk hands the resolver — the depth split changes only
/// the version pick, and the per-edge overrides change only
/// `project_dir` / `current_pkg` / the overlay — so [`TreeCtx`](crate::TreeCtx) builds
/// this once per importer and every edge shares it.
/// [`Self::matches_options`] backs the debug assertion pinning that
/// invariance.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(in super::super) struct WorkspaceResolutionOptionsKey {
    pub(super) workspace_packages: Option<WorkspacePackagesKey>,
    pub(super) lockfile_dir: PathKey,
    pub(super) default_tag: Option<String>,
    pub(super) inject_workspace_packages: bool,
    pub(super) calc_specifier: bool,
    pub(super) range_spec_style_discriminant: Option<u8>,
    pub(super) save_workspace_protocol_discriminant: u8,
}

impl WorkspaceResolutionOptionsKey {
    pub(in super::super) fn new(options: &ResolveOptions) -> Self {
        Self {
            workspace_packages: options.workspace_packages.as_ref().map(WorkspacePackagesKey::new),
            lockfile_dir: PathKey(options.lockfile_dir.clone()),
            default_tag: options.default_tag.clone(),
            inject_workspace_packages: options.inject_workspace_packages,
            calc_specifier: options.calc_specifier,
            range_spec_style_discriminant: options.range_spec_style.map(|style| style as u8),
            save_workspace_protocol_discriminant: options.save_workspace_protocol as u8,
        }
    }

    /// Whether the importer-wide key still describes `options` — the
    /// per-importer invariance the shared cache relies on, asserted at
    /// the key's use site in debug builds.
    #[cfg(debug_assertions)]
    pub(in super::super) fn matches_options(&self, options: &ResolveOptions) -> bool {
        *self == Self::new(options)
    }
}

/// Keeps the immutable workspace map alive.
/// The key uses pointer identity instead of hashing the map for every dependency edge.
/// Clones of the same [`Arc`] share a key. Separately allocated maps use separate keys.
#[derive(Clone)]
pub(super) struct WorkspacePackagesKey(Arc<WorkspacePackages>);

impl WorkspacePackagesKey {
    pub(super) fn new(packages: &Arc<WorkspacePackages>) -> Self {
        Self(Arc::clone(packages))
    }
}

impl std::fmt::Debug for WorkspacePackagesKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("WorkspacePackagesKey").field(&Arc::as_ptr(&self.0)).finish()
    }
}

impl PartialEq for WorkspacePackagesKey {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for WorkspacePackagesKey {}

impl Hash for WorkspacePackagesKey {
    fn hash<State: Hasher>(&self, state: &mut State) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

/// Cache key for a hook-processed workspace result.
#[derive(Debug, PartialEq, Eq, Hash)]
pub(in super::super) struct WorkspaceFinalWantedKey {
    pub(super) shared_wanted: SharedWorkspaceWantedKey,
    pub(super) canonical_resolution_id: PkgResolutionId,
    pub(super) rendered_resolution_id: PkgResolutionId,
}

impl WorkspaceFinalWantedKey {
    pub(in super::super) fn new(
        shared_wanted: SharedWorkspaceWantedKey,
        canonical_resolution_id: &PkgResolutionId,
        rendered_resolution_id: &PkgResolutionId,
    ) -> Self {
        Self {
            shared_wanted,
            canonical_resolution_id: canonical_resolution_id.clone(),
            rendered_resolution_id: rendered_resolution_id.clone(),
        }
    }
}
