use pnpm_lockfile::{PackageKey, SnapshotEntry};
use std::collections::{HashMap, HashSet};

/// The set of snapshot keys skipped on this host.
///
/// Disjoint origin classes are tracked separately because
/// they behave differently across installs:
///
/// - **Installability skips** (`installability`) — engine, platform,
///   or libc mismatch surfaced by [`compute_skipped_snapshots`].
///   Persisted to `.modules.yaml.skipped` and re-seeded on every
///   subsequent install.
///
/// - **Fetch-failure skips** (`fetch_failed`) — an `optional: true`
///   snapshot whose tarball / metadata / extract step blew up
///   during the install. **Not** persisted: the catch site never
///   records the skip, so a subsequent install retries the fetch.
///
/// - **`--no-optional` exclusions** (`optional_excluded`) —
///   snapshots whose lockfile entry has `optional: true` AND the
///   user passed `--no-optional` (or `IncludedDependencies::optional_dependencies`
///   is false). **Not** persisted: the filter sits downstream of the
///   skip set, so re-running without
///   `--no-optional` brings the snapshots back into the install
///   graph. Pacquet's downstream architecture walks the lockfile
///   directly rather than a pre-pruned graph, so a separate filter
///   is needed here.
///
/// All subsets contribute to [`contains`] and [`iter`] —
/// downstream walkers treat skipped-for-any-reason uniformly. Only
/// the `installability` subset survives [`iter_installability`],
/// which is what `.modules.yaml.skipped` writes.
///
/// [`compute_skipped_snapshots`]: crate::compute_skipped_snapshots
/// [`contains`]: SkippedSnapshots::contains
/// [`iter`]: SkippedSnapshots::iter
/// [`iter_installability`]: SkippedSnapshots::iter_installability
#[derive(Debug, Default, Clone)]
pub struct SkippedSnapshots {
    installability: HashSet<PackageKey>,
    fetch_failed: HashSet<PackageKey>,
    optional_excluded: HashSet<PackageKey>,
}

impl SkippedSnapshots {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a [`SkippedSnapshots`] from an existing
    /// installability set. Test helper for callers that want to
    /// drive build-sequence / virtual-store filtering against a
    /// known skip set without running the full installability pass.
    #[must_use]
    pub fn from_set(set: HashSet<PackageKey>) -> Self {
        Self { installability: set, ..Self::default() }
    }

    /// Seed the installability set with snapshot keys recorded as
    /// skipped by a previous install (read from
    /// `.modules.yaml.skipped`). Unparsable strings are silently
    /// dropped — the seed is only consulted by membership lookup; a
    /// nonsense string never matches any current snapshot, so the
    /// orphan is harmless.
    pub fn from_strings<Iter>(iter: Iter) -> Self
    where
        Iter: IntoIterator,
        Iter::Item: AsRef<str>,
    {
        let installability = iter
            .into_iter()
            .filter_map(|text| text.as_ref().parse::<PackageKey>().ok())
            .collect();
        Self { installability, ..Self::default() }
    }

    /// Record an `optional: true` snapshot whose fetch / extract
    /// failed during this install.
    ///
    /// Disjoint-subset guard: if `key` is already in any other
    /// subset, the insert is a no-op so [`len`] / [`iter`] stay
    /// consistent with [`contains`]. In practice the only
    /// realistic overlap is with `installability`
    /// (`optional_excluded` snapshots are dropped before reaching
    /// the cold-batch dispatch), but the guard is symmetric with
    /// [`add_optional_excluded`]'s so the public API enforces the
    /// invariant regardless of call order.
    ///
    /// [`len`]: SkippedSnapshots::len
    /// [`iter`]: SkippedSnapshots::iter
    /// [`contains`]: SkippedSnapshots::contains
    /// [`add_optional_excluded`]: SkippedSnapshots::add_optional_excluded
    pub fn add_fetch_failed(&mut self, key: PackageKey) {
        if self.installability.contains(&key) || self.optional_excluded.contains(&key) {
            return;
        }
        self.fetch_failed.insert(key);
    }

    pub fn add_fetch_failed_all(&mut self, keys: impl IntoIterator<Item = PackageKey>) {
        for key in keys {
            self.add_fetch_failed(key);
        }
    }

    /// Record a snapshot dropped because the user passed
    /// `--no-optional` (or the matching config / `IncludedDependencies`
    /// flag is false). Downstream gates then drop the snapshot from
    /// extraction, symlinking, building, and hoisting through the
    /// same skip-set check they use for installability skips.
    ///
    /// Disjoint-subset guard: a snapshot that is both
    /// installability-skipped (platform / engine mismatch) and
    /// would-be excluded by `--no-optional` stays in the
    /// higher-precedence `installability` subset only. This is the
    /// realistic overlap case (an `optional: true` snapshot that's
    /// also `os: [<wrong>]`), and putting it in both subsets would
    /// make [`len`] / [`iter`] inconsistent with [`contains`].
    /// Same guard applies against `fetch_failed`, though that
    /// overlap can't arise in practice (a snapshot dropped by
    /// `--no-optional` never reaches the cold-batch dispatch).
    ///
    /// [`len`]: SkippedSnapshots::len
    /// [`iter`]: SkippedSnapshots::iter
    /// [`contains`]: SkippedSnapshots::contains
    pub fn add_optional_excluded(&mut self, key: PackageKey) {
        if self.installability.contains(&key) || self.fetch_failed.contains(&key) {
            return;
        }
        self.optional_excluded.insert(key);
    }

    /// `true` if the snapshot is skipped for **any** reason
    /// (installability, fetch-failure, or `--no-optional`).
    /// Downstream consumers want the union: a dropped snapshot is
    /// equally absent from the install regardless of origin.
    #[must_use]
    pub fn contains(&self, key: &PackageKey) -> bool {
        self.installability.contains(key)
            || self.fetch_failed.contains(key)
            || self.optional_excluded.contains(key)
    }

    #[must_use]
    pub fn contains_optional_excluded(&self, key: &PackageKey) -> bool {
        self.optional_excluded.contains(key)
    }

    /// Whether [`Self::contains`] and [`Self::contains_optional_excluded`]
    /// answer alike for every key, which they do exactly when neither the
    /// installability nor the fetch-failure subset holds anything. A caller
    /// that would otherwise walk the graph once per predicate can then walk
    /// it once.
    #[must_use]
    pub fn optional_exclusions_are_the_only_skips(&self) -> bool {
        self.installability.is_empty() && self.fetch_failed.is_empty()
    }

    /// The same set with the `installability` subset dropped — the
    /// skips a written current lockfile has to reflect.
    ///
    /// `.modules.yaml.skipped` carries the installability subset from
    /// one install to the next, so the current lockfile can keep those
    /// entries and still describe what is on disk. It has to keep
    /// them: pnpm's current lockfile does, and a repeat install's
    /// "already up to date" comparison against the wanted lockfile
    /// would otherwise never match again once a platform-incompatible
    /// optional dependency was skipped. Fetch failures and
    /// `--no-optional` exclusions are recorded nowhere else, so
    /// leaving those out is what makes the next install redo them.
    #[must_use]
    pub fn transient_only(&self) -> Self {
        Self {
            installability: HashSet::new(),
            fetch_failed: self.fetch_failed.clone(),
            optional_excluded: self.optional_excluded.clone(),
        }
    }

    /// The same set with the `--no-optional` exclusions dropped, for walks of
    /// unselected importers whose optional dependencies remain installed.
    #[must_use]
    pub fn without_optional_excluded(&self) -> Self {
        Self {
            installability: self.installability.clone(),
            fetch_failed: self.fetch_failed.clone(),
            optional_excluded: HashSet::new(),
        }
    }

    pub fn retain_installability_for_optional_snapshots(
        &mut self,
        snapshots: &HashMap<PackageKey, SnapshotEntry>,
    ) {
        self.installability.retain(|key| {
            snapshots.get(key).is_some_and(|snapshot| snapshot.optional)
        });
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.installability.len()
            + self.fetch_failed.len()
            + self.optional_excluded.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.installability.is_empty()
            && self.fetch_failed.is_empty()
            && self.optional_excluded.is_empty()
    }

    /// Insert into the installability set — the persisted subset
    /// written to `.modules.yaml.skipped`.
    pub fn insert_installability(&mut self, key: PackageKey) {
        self.fetch_failed.remove(&key);
        self.optional_excluded.remove(&key);
        self.installability.insert(key);
    }

    #[must_use]
    pub fn contains_installability(&self, key: &PackageKey) -> bool {
        self.installability.contains(key)
    }

    /// Drop a seeded installability skip whose package passes the
    /// current installability check — e.g. after `--os` / `--cpu` /
    /// `supportedArchitectures` changed between installs.
    pub fn remove_installability(&mut self, key: &PackageKey) {
        self.installability.remove(key);
    }

    /// Iterate over the **installability** subset only — the entries
    /// written to `.modules.yaml.skipped`. Fetch-failure and
    /// `--no-optional` entries are transient and intentionally
    /// excluded so they aren't persisted across installs.
    pub fn iter_installability(&self) -> impl Iterator<Item = &PackageKey> + '_ {
        self.installability.iter()
    }

    /// Iterate over the union of all subsets — every snapshot that
    /// downstream consumers should treat as absent from the install,
    /// regardless of origin. Used by `hoist.rs` and similar
    /// graph-walking passes that don't care why a snapshot is gone.
    pub fn iter(&self) -> impl Iterator<Item = &PackageKey> + '_ {
        self.installability
            .iter()
            .chain(self.fetch_failed.iter())
            .chain(self.optional_excluded.iter())
    }
}
