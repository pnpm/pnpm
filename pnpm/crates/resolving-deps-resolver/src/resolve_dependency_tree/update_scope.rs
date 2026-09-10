use super::{BTreeMap, BTreeSet};

/// Which dependencies `pacquet update` excludes from lockfile-resolution
/// reuse. An excluded package re-resolves to highest-in-range, and its
/// whole subtree re-resolves with it (so the bump's new transitive deps
/// are picked up).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum UpdateReuseScope {
    /// Reuse every still-satisfied dependency. `install` / `add`.
    #[default]
    All,
    /// Reuse nothing — the whole graph re-resolves. `pacquet update`
    /// with no selectors.
    None,
    /// Reuse everything except the update's targets (matched at any depth
    /// the update reaches). `pacquet update <pattern>`.
    Except(UpdateTargets),
}

/// The major and minor of a version, which is how far an exact update
/// selector reaches. `pacquet update foo@1.2.3` targets only the copies of
/// `foo` that could resolve to `1.2.3`: the same major, or -- on `0.x`,
/// where the minor is the compatibility boundary -- the same minor. Copies
/// on another line keep their locked resolution, so bumping one line of a
/// package the workspace depends on twice leaves the other alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VersionLine {
    pub(super) major: u64,
    pub(super) minor: u64,
}

impl VersionLine {
    /// The line `version` sits on.
    #[must_use]
    pub fn of(version: &node_semver::Version) -> Self {
        VersionLine { major: version.major, minor: version.minor }
    }

    /// The line a version selector pins, or `None` when it pins none -- a
    /// range, a tag and an `npm:` alias spec all name no single version.
    #[must_use]
    pub fn parse(version_spec: &str) -> Option<Self> {
        node_semver::Version::parse(version_spec).ok().as_ref().map(VersionLine::of)
    }

    /// Whether `version` resolves within this line.
    #[must_use]
    pub(super) fn covers(self, version: &node_semver::Version) -> bool {
        version.major == self.major && (self.major != 0 || version.minor == self.minor)
    }
}

/// The packages a `pacquet update` targets, each mapped to the version
/// lines its selectors scoped it to -- or to `None` when a selector named
/// no single version, which targets the package at every version. See
/// [`VersionLine`].
#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UpdateTargets(BTreeMap<String, Option<BTreeSet<VersionLine>>>);

impl UpdateTargets {
    /// Add `name` as a target. `line` scopes it to one version line; `None`
    /// widens the target to every version, and never narrows one already
    /// recorded.
    pub fn insert(&mut self, name: String, line: Option<VersionLine>) {
        let lines = self.0.entry(name).or_insert_with(|| Some(BTreeSet::new()));
        match line {
            // pnpm evaluates every selector that matches a dependency, so
            // one selector targeting every version makes the narrower ones
            // moot.
            None => *lines = None,
            Some(line) => {
                if let Some(lines) = lines {
                    lines.insert(line);
                }
            }
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether the edge resolved to `version` under `name` is an update
    /// target. A `None` version -- an edge with no locked resolution to
    /// judge yet -- matches by name alone, mirroring pnpm's version-less
    /// `updateMatching` calls.
    #[must_use]
    pub fn covers(&self, name: &str, version: Option<&node_semver::Version>) -> bool {
        let Some(lines) = self.0.get(name) else { return false };
        let (Some(lines), Some(version)) = (lines.as_ref(), version) else { return true };
        lines.iter().any(|line| line.covers(version))
    }
}

impl FromIterator<(String, Option<VersionLine>)> for UpdateTargets {
    fn from_iter<Iter: IntoIterator<Item = (String, Option<VersionLine>)>>(iter: Iter) -> Self {
        let mut targets = UpdateTargets::default();
        for (name, line) in iter {
            targets.insert(name, line);
        }
        targets
    }
}

/// How deep `pacquet update` reaches — the `--depth` ceiling. A node
/// below it keeps its locked resolution even when its name is an update
/// target, matching pnpm's `currentDepth <= updateDepth` gate.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct UpdateDepth(Option<i32>);

impl UpdateDepth {
    /// `--depth Infinity`, the default.
    pub const UNLIMITED: Self = Self(None);

    /// A depth no dependency graph can reach — `usize::MAX`, which the
    /// CLI uses for the `Infinity` default, among them — is unlimited.
    #[must_use]
    pub fn new(depth: usize) -> Self {
        i32::try_from(depth).map_or(Self::UNLIMITED, |depth| Self(Some(depth)))
    }

    /// Whether an update reaches a node at `depth`.
    #[must_use]
    pub(super) fn reaches(self, depth: i32) -> bool {
        self.0.is_none_or(|max_depth| depth <= max_depth)
    }

    /// The depth to memoise a subtree-reuse answer under. Beyond the
    /// ceiling no node is an update target, so every deeper level shares
    /// one answer — and an unlimited update never varies by depth at all.
    #[must_use]
    pub(super) fn memo_bucket(self, depth: i32) -> i32 {
        match self.0 {
            None => 0,
            Some(max_depth) => depth.min(max_depth.saturating_add(1)),
        }
    }
}
