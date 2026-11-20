//! Deciding which packages are allowed to run build scripts.

pub use pnpm_package_name::{
    git_repo_allow_build_key_from_dep_path,
    is_git_hosted_dep_path,
    parse_dep_path_name_version,
    short_package_label,
};

use super::{
    Config,
    HashMap,
    HashSet,
    PackageKey,
    PackageVersionPolicy,
    PolicyMatch,
    VersionPolicyError,
    create_package_version_policy,
    expand_package_version_specs,
    get_pkg_id_with_patch_hash,
    parse_name_version_from_key,
    remove_suffix,
};
use pnpm_package_name::is_git_repo_dep_path;

/// Build policy derived from `allowBuilds` and
/// `dangerouslyAllowAllBuilds` in `pnpm-workspace.yaml`.
///
/// The internal `expanded_allowed` and `expanded_disallowed` sets
/// contain the result of running each `allowBuilds` key through
/// [`expand_package_version_specs`], so a key like
/// `foo@1.0.0 || 2.0.0` lands as two separate `foo@1.0.0` and
/// `foo@2.0.0` entries that [`AllowBuildPolicy::check`] can match
/// via `HashSet::contains`.
#[derive(Debug, Default)]
pub struct AllowBuildPolicy {
    expanded_allowed: HashSet<String>,
    expanded_disallowed: HashSet<String>,
    allowed_dep_paths: HashSet<String>,
    disallowed_dep_paths: HashSet<String>,
    allowed_git_repos: HashSet<String>,
    disallowed_git_repos: HashSet<String>,
    dangerously_allow_all: bool,
    side_effects_cache_exclude: Option<PackageVersionPolicy>,
}

impl AllowBuildPolicy {
    /// Build a policy from already-expanded `allowed` and
    /// `disallowed` sets and `dangerouslyAllowAllBuilds`. Pure
    /// constructor (no IO) so the policy logic is tested
    /// directly with in-memory inputs.
    #[must_use]
    pub fn new(
        expanded_allowed: HashSet<String>,
        expanded_disallowed: HashSet<String>,
        dangerously_allow_all: bool,
    ) -> Self {
        Self {
            expanded_allowed,
            expanded_disallowed,
            allowed_dep_paths: HashSet::new(),
            disallowed_dep_paths: HashSet::new(),
            allowed_git_repos: HashSet::new(),
            disallowed_git_repos: HashSet::new(),
            dangerously_allow_all,
            side_effects_cache_exclude: None,
        }
    }

    #[must_use]
    pub fn new_with_dep_paths(
        expanded_allowed: HashSet<String>,
        expanded_disallowed: HashSet<String>,
        allowed_dep_paths: HashSet<String>,
        disallowed_dep_paths: HashSet<String>,
        dangerously_allow_all: bool,
    ) -> Self {
        Self {
            expanded_allowed,
            expanded_disallowed,
            allowed_dep_paths,
            disallowed_dep_paths,
            allowed_git_repos: HashSet::new(),
            disallowed_git_repos: HashSet::new(),
            dangerously_allow_all,
            side_effects_cache_exclude: None,
        }
    }

    /// Build the policy from a resolved [`Config`]. Reads
    /// `allow_builds` and `dangerously_allow_all_builds`, which are
    /// populated by [`pnpm_config::WorkspaceSettings::apply_to`]
    /// from `pnpm-workspace.yaml`. pnpm v11 stopped reading these
    /// from `package.json#pnpm`; see pnpm/pacquet#397 item 5.
    pub fn from_config(config: &Config) -> Result<Self, VersionPolicyError> {
        Self::from_approvals(&config.allow_builds, config.dangerously_allow_all_builds)?
            .with_side_effects_cache_exclude(
                config.side_effects_cache_exclude.as_deref().unwrap_or_default(),
            )
    }

    /// Build the policy from a map of `allowBuilds`-shaped keys to
    /// decisions. The same keys decide every capability of `permissions`,
    /// so the policy for agent skills is built here from
    /// [`Config::allow_skills`].
    pub fn from_approvals(
        approvals: &HashMap<String, bool>,
        dangerously_allow_all: bool,
    ) -> Result<Self, VersionPolicyError> {
        let mut allowed = BuildKeys::default();
        let mut disallowed = BuildKeys::default();
        for (spec, &value) in approvals {
            let keys = if value { &mut allowed } else { &mut disallowed };
            keys.add(spec);
        }
        Ok(Self::new_with_dep_paths(
            expand_package_version_specs(allowed.specs)?,
            expand_package_version_specs(disallowed.specs)?,
            allowed.dep_paths,
            disallowed.dep_paths,
            dangerously_allow_all,
        )
        .with_git_repo_rules(allowed.git_repos, disallowed.git_repos))
    }

    /// Add the `sideEffectsCache.exclude` rules [`Self::caches_build`]
    /// reads.
    pub fn with_side_effects_cache_exclude(
        mut self,
        patterns: &[String],
    ) -> Result<Self, VersionPolicyError> {
        self.side_effects_cache_exclude =
            if patterns.is_empty() { None } else { Some(create_package_version_policy(patterns)?) };
        Ok(self)
    }

    /// Whether the build of the snapshot at `snapshot_key` may be
    /// restored from, and saved to, the side-effects cache and shared
    /// through a global-virtual-store slot. `false` for a package
    /// `sideEffectsCache.exclude` names.
    ///
    /// A bare-name rule covers every resolution of the package, a git or
    /// tarball URL included. A `name@version` rule names registry
    /// versions only, from the default registry or a named one.
    #[must_use]
    pub fn caches_build(&self, snapshot_key: &PackageKey) -> bool {
        let Some(exclude) = &self.side_effects_cache_exclude else {
            return true;
        };
        match exclude.matches(&snapshot_key.name.to_string()) {
            PolicyMatch::No => true,
            PolicyMatch::AnyVersion => false,
            PolicyMatch::ExactVersions(versions) => !snapshot_key.suffix
                .version_semver()
                .or_else(|| snapshot_key.suffix.registry_qualified().map(|(_, version)| version))
                .is_some_and(|version| {
                    versions
                        .iter()
                        .any(|exact| exact == &version.to_string())
                }),
        }
    }

    #[must_use]
    fn with_git_repo_rules(
        mut self,
        allowed_git_repos: HashSet<String>,
        disallowed_git_repos: HashSet<String>,
    ) -> Self {
        self.allowed_git_repos = allowed_git_repos;
        self.disallowed_git_repos = disallowed_git_repos;
        self
    }

    /// Check whether a package is allowed to run build scripts. An
    /// explicit `false` in `allowBuilds` outranks
    /// `dangerouslyAllowAllBuilds`, so the two together allow every
    /// build except the denied ones.
    #[must_use]
    pub fn check(&self, dep_path: &str) -> Option<bool> {
        if self.dangerously_allow_all && !self.has_denials() {
            return Some(true);
        }
        let normalized_dep_path = normalize_build_dep_path(dep_path);
        let git_repo_key = git_repo_allow_build_key_from_dep_path(&normalized_dep_path);
        let git_repo_key = git_repo_key.as_deref();
        let (name, version) = parse_name_version_from_key(&normalized_dep_path);
        let name_at_version = format!("{name}@{version}");
        if self.denies(&normalized_dep_path, git_repo_key, (&name, &name_at_version)) {
            return Some(false);
        }
        if self.dangerously_allow_all {
            return Some(true);
        }
        if self.allowed_dep_paths.contains(&normalized_dep_path)
            || git_repo_key.is_some_and(|key| self.allowed_git_repos.contains(key))
        {
            return Some(true);
        }
        // Package-name rules require a trusted package identity. A
        // registry-style dep path (`name@semver`) is the trust signal: the
        // lockfile verification gate rejects lockfiles where such a key is
        // backed by a non-registry resolution, so by the time scripts can
        // run, the shape proves the artifact came from a registry.
        if node_semver::Version::parse(&version).is_err() {
            return None;
        }
        if self.expanded_allowed.contains(&name) || self.expanded_allowed.contains(&name_at_version)
        {
            return Some(true);
        }

        None
    }

    fn has_denials(&self) -> bool {
        !(self.disallowed_dep_paths.is_empty()
            && self.disallowed_git_repos.is_empty()
            && self.expanded_disallowed.is_empty())
    }

    /// A denial by dep path, git repo, or package name outranks every
    /// allowance.
    fn denies(
        &self,
        normalized_dep_path: &str,
        git_repo_key: Option<&str>,
        named: (&str, &str),
    ) -> bool {
        let (name, name_at_version) = named;
        self.disallowed_dep_paths.contains(normalized_dep_path)
            || git_repo_key.is_some_and(|key| self.disallowed_git_repos.contains(key))
            || self.expanded_disallowed.contains(name)
            || self.expanded_disallowed.contains(name_at_version)
    }
}

/// Strips the peer suffix (and, matching [`PkgVerPeer::without_peer`]'s
/// lumped suffix handling, the patch hash) so config keys compare equal
/// to the `metadata_key.to_string()` form used at the runtime call sites.
///
/// [`PkgVerPeer::without_peer`]: pnpm_lockfile::PkgVerPeer::without_peer
#[must_use]
pub fn normalize_build_dep_path(dep_path: &str) -> String {
    if is_link_dep_path(dep_path) {
        return dep_path.to_string();
    }
    remove_suffix(dep_path).to_string()
}

/// The `allowBuilds` key under which an ignored build should be approved:
/// the package name for registry packages, the peer-suffix-free depPath for
/// git/tarball artifacts (whose name alone must not approve builds).
#[must_use]
pub fn allow_build_key_from_ignored_build(dep_path: &str) -> String {
    if is_link_dep_path(dep_path) {
        return dep_path.to_string();
    }
    let pkg_id_with_patch_hash = get_pkg_id_with_patch_hash(dep_path);
    match parse_dep_path_name_version(pkg_id_with_patch_hash) {
        Some((name, version)) if node_semver::Version::parse(version).is_ok() => name.to_string(),
        _ => pkg_id_with_patch_hash.to_string(),
    }
}

fn is_link_dep_path(dep_path: &str) -> bool {
    parse_dep_path_name_version(dep_path).is_some_and(|(_, version)| version.starts_with("link:"))
}

/// The package an `--allow-build` value or an `approve-builds` argument
/// names, and whether it is allowed to build: a leading `!` denies the
/// build.
///
/// A selector that is empty or only `!` yields an empty name. Callers
/// reject that rather than persist an empty `allowBuilds` key.
#[must_use]
pub fn parse_allow_build_selector(selector: &str) -> (&str, bool) {
    match selector.strip_prefix('!') {
        Some(name) => (name, false),
        None => (selector, true),
    }
}

/// One side of `allowBuilds`, split by the shape of its keys.
#[derive(Default)]
struct BuildKeys<'c> {
    specs: Vec<&'c str>,
    dep_paths: HashSet<String>,
    git_repos: HashSet<String>,
}

impl<'c> BuildKeys<'c> {
    fn add(&mut self, spec: &'c str) {
        if is_git_repo_allow_build_key(spec) {
            self.git_repos.insert(spec.to_owned());
        } else if is_dep_path_allow_build_key(spec) {
            self.dep_paths.insert(normalize_build_dep_path(spec));
        } else {
            self.specs.push(spec);
        }
    }
}

pub(crate) fn is_git_repo_allow_build_key(spec: &str) -> bool {
    !spec.contains('#') && is_git_repo_dep_path(spec)
}

pub(crate) fn is_dep_path_allow_build_key(spec: &str) -> bool {
    if normalize_build_dep_path(spec) != spec {
        return true;
    }
    if spec.contains("||") {
        return false;
    }
    let Some((_, version)) = parse_dep_path_name_version(spec) else {
        return !spec.starts_with('@') && (spec.contains('/') || spec.contains(':'));
    };
    node_semver::Version::parse(version).is_err() && is_source_like_dep_path_version(version)
}

pub(crate) fn is_source_like_dep_path_version(version: &str) -> bool {
    version.contains(':') || version.contains('/') || version.contains('#')
}

#[cfg(test)]
mod tests;
