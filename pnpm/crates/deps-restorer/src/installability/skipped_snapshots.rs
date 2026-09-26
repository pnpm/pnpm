use pnpm_lockfile::{PackageKey, SnapshotEntry};
use std::collections::{HashMap, HashSet};

/// The set of snapshot keys skipped on this host.
///
/// Three disjoint origin classes are tracked separately because
/// they behave differently across installs:
///
/// - **Installability skips** (`installability`) — engine, platform,
///   or libc mismatch surfaced by [`compute_skipped_snapshots`](super::compute_skipped_snapshots).
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
/// All three subsets contribute to [`contains`](Self::contains) and [`iter`](Self::iter) —
/// downstream walkers treat skipped-for-any-reason uniformly. Only
/// the `installability` subset survives [`iter_installability`](Self::iter_installability),
/// which is what `.modules.yaml.skipped` writes.
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
    /// failed during this install. Slice 4 wire-up — call site is
    /// inside [`crate::CreateVirtualStore`]'s cold-batch dispatch.
    ///
    /// Disjoint-subset guard: if `key` is already in any other
    /// subset, the insert is a no-op so [`len`](Self::len) / [`iter`](Self::iter) stay
    /// consistent with [`contains`](Self::contains). In practice the only
    /// realistic overlap is with `installability`
    /// (`optional_excluded` snapshots are dropped before reaching
    /// the cold-batch dispatch), but the guard is symmetric with
    /// [`add_optional_excluded`](Self::add_optional_excluded)'s so the public API enforces the
    /// invariant regardless of call order.
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

    /// Record snapshots that a package provider failed to materialize.
    pub fn add_provider_failed(&mut self, keys: impl IntoIterator<Item = PackageKey>) {
        for key in keys {
            self.fetch_failed.remove(&key);
            self.optional_excluded.remove(&key);
            self.installability.insert(key);
        }
    }

    /// Record a snapshot dropped because the user passed
    /// `--no-optional` (or the matching config / `IncludedDependencies`
    /// flag is false). Slice 5 wire-up — call site is inside
    /// `InstallFrozenLockfile::run`, which iterates the lockfile
    /// snapshots once and inserts every `snap.optional == true`
    /// entry. Downstream gates then drop the snapshot from
    /// extraction, symlinking, building, and hoisting through the
    /// same skip-set check they use for installability skips.
    ///
    /// Disjoint-subset guard: a snapshot that is both
    /// installability-skipped (platform / engine mismatch) and
    /// would-be excluded by `--no-optional` stays in the
    /// higher-precedence `installability` subset only. This is the
    /// realistic overlap case (an `optional: true` snapshot that's
    /// also `os: [<wrong>]`), and putting it in both subsets would
    /// make [`len`](Self::len) / [`iter`](Self::iter) inconsistent with [`contains`](Self::contains).
    /// Same guard applies against `fetch_failed`, though that
    /// overlap can't arise in practice (a snapshot dropped by
    /// `--no-optional` never reaches the cold-batch dispatch).
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

    /// Return a copy of this set carrying only the transient subsets
    /// (`fetch_failed` and `optional_excluded`), dropping the
    /// persisted `installability` skips.
    ///
    /// Slices 5/6 wire-up — `MaterializationPlan`'s cache
    /// fingerprint needs the transient exclusions so `--no-optional`
    /// partitions the cache, but cannot include `installability` skips:
    /// the cached fingerprint was computed before the installability
    /// pass ran, so comparing against the full [`SkippedSnapshots`]
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
