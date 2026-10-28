use super::{
    BTreeMap, Command, FsWalkFiles, PackageBin, PathBuf, Value, commands_from_precalculated_bins,
    get_bins_from_package_manifest,
};
use std::sync::Arc;

/// One package known to be installed at `location`, with its parsed
/// `package.json`. The per-package input to [`super::link_bins_of_packages`].
///
/// The manifest is shared via `Arc` rather than owned by value: the
/// lockfile-driven bin-link path looks up the same parsed manifest
/// from a process-wide map, so packing it into a [`PackageBinSource`]
/// is a refcount bump (cheap) rather than a deep clone of the JSON
/// tree (which would have been the bulk of the per-slot CPU work,
/// since the per-install clone count is `slots × children` =
/// thousands of times).
#[derive(Debug, Clone)]
pub struct PackageBinSource {
    pub location: PathBuf,
    pub manifest: Arc<Value>,
    /// Where this candidate came from. When a hoisted (transitive)
    /// dep and a direct dep both declare the same bin name, the
    /// direct dep must win so a project never gets its own tooling
    /// silently shadowed by a transitive's bin. Defaults to
    /// [`BinOrigin::Direct`] —
    /// constructions via [`PackageBinSource::new`] don't have to
    /// supply the field.
    pub origin: BinOrigin,
    /// The package directory with every symlink resolved — the
    /// virtual-store slot dir a symlinked [`location`] points at.
    /// Callers that already know it (the lockfile-driven passes
    /// derive it from the snapshot key and the store layout) set it so
    /// the shim `NODE_PATH` derives lexically; when `None` the linker
    /// falls back to one `canonicalize` per package.
    ///
    /// [`location`]: Self::location
    pub resolved_location: Option<PathBuf>,
    /// Whether the package's lifecycle scripts have yet to run in this
    /// install. Such a package's bin is not linked while its target is
    /// missing, because the scripts may create it and run with this
    /// `.bin` on `PATH`. The linking pass after the build links it.
    pub build_pending: bool,
    /// Precalculated bin targets and shebang runtimes from `dist.pnpr.bins` or
    /// archive extraction. Used when `build_pending` is `false` to skip on-disk
    /// `directories.bin` directory walks, missing-target head reads, and
    /// shebang reads.
    pub precalculated_bins: Option<Arc<BTreeMap<String, PackageBin>>>,
}

impl PackageBinSource {
    /// Construct a [`PackageBinSource`] tagged as
    /// [`BinOrigin::Direct`]. Use this for direct-dependency
    /// candidates and for any call site that doesn't need to
    /// distinguish direct from hoisted (per-slot bin linking,
    /// most tests).
    #[must_use]
    pub fn new(location: PathBuf, manifest: Arc<Value>) -> Self {
        Self {
            location,
            manifest,
            origin: BinOrigin::Direct,
            resolved_location: None,
            build_pending: false,
            precalculated_bins: None,
        }
    }

    /// Tag this source with the given [`BinOrigin`].
    #[must_use]
    pub fn with_origin(mut self, origin: BinOrigin) -> Self {
        self.origin = origin;
        self
    }

    /// Record the symlink-resolved package directory. See
    /// [`Self::resolved_location`].
    #[must_use]
    pub fn with_resolved_location(mut self, resolved_location: PathBuf) -> Self {
        self.resolved_location = Some(resolved_location);
        self
    }

    /// Mark the package's lifecycle scripts as not yet run. See
    /// [`Self::build_pending`].
    #[must_use]
    pub fn with_build_pending(mut self, build_pending: bool) -> Self {
        self.build_pending = build_pending;
        self
    }

    /// Attach precalculated bin metadata (`dist.pnpr.bins` or archive-extracted
    /// bins).
    #[must_use]
    pub fn with_precalculated_bins(
        mut self,
        precalculated_bins: Option<Arc<BTreeMap<String, PackageBin>>>,
    ) -> Self {
        self.precalculated_bins = precalculated_bins;
        self
    }

    pub(crate) fn resolve_commands<Sys: FsWalkFiles>(&self) -> Vec<Command> {
        if !self.build_pending
            && let Some(bins) = &self.precalculated_bins
            && let Some(commands) = commands_from_precalculated_bins(bins, &self.location)
        {
            return commands;
        }
        get_bins_from_package_manifest::<Sys>(&self.manifest, &self.location)
    }
}

/// Origin of a [`PackageBinSource`] determining precedence during conflict resolution:
/// direct dependencies take precedence over publicly hoisted packages, which take precedence
/// over auto-installed peers.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BinOrigin {
    /// The candidate is a direct dependency of the importer
    /// installing it. Direct deps come from the per-importer
    /// `dependencies` / `devDependencies` / `optionalDependencies`
    /// maps in the lockfile / manifest.
    #[default]
    Direct,
    /// The candidate is a transitive dependency that the hoister
    /// lifted to a top-level (or per-`node_modules`) slot. Bins
    /// from these candidates are dropped when a same-named
    /// [`Self::Direct`] candidate is also present.
    Hoisted,
    /// An auto-installed peer exposed to a project; its bin must not
    /// replace a directly installed or publicly hoisted command.
    Peer,
}
