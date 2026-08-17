//! Deciding which packages are allowed to run build scripts.

use super::{
    Config, Cow, HashSet, VersionPolicyError, expand_package_version_specs,
    get_pkg_id_with_patch_hash, index_of_dep_path_suffix, parse_name_version_from_key,
    remove_suffix,
};

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
}

impl AllowBuildPolicy {
    /// Build a policy from already-expanded `allowed` and
    /// `disallowed` sets and `dangerouslyAllowAllBuilds`. Pure
    /// constructor — no IO — so the policy logic is tested
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
        }
    }

    /// Build the policy from a resolved [`Config`]. Reads
    /// `allow_builds` and `dangerously_allow_all_builds`, which are
    /// populated by [`pnpm_config::WorkspaceSettings::apply_to`]
    /// from `pnpm-workspace.yaml`. pnpm v11 stopped reading these
    /// from `package.json#pnpm` — see pnpm/pacquet#397 item 5.
    pub fn from_config(config: &Config) -> Result<Self, VersionPolicyError> {
        let mut allowed_specs: Vec<&str> = Vec::new();
        let mut disallowed_specs: Vec<&str> = Vec::new();
        let mut allowed_dep_paths = HashSet::new();
        let mut disallowed_dep_paths = HashSet::new();
        let mut allowed_git_repos = HashSet::new();
        let mut disallowed_git_repos = HashSet::new();
        for (spec, &value) in &config.allow_builds {
            if is_git_repo_allow_build_key(spec) {
                if value {
                    allowed_git_repos.insert(spec.clone());
                } else {
                    disallowed_git_repos.insert(spec.clone());
                }
            } else if is_dep_path_allow_build_key(spec) {
                if value {
                    allowed_dep_paths.insert(normalize_build_dep_path(spec));
                } else {
                    disallowed_dep_paths.insert(normalize_build_dep_path(spec));
                }
            } else {
                if value {
                    allowed_specs.push(spec);
                } else {
                    disallowed_specs.push(spec);
                }
            }
        }
        let expanded_allowed = expand_package_version_specs(allowed_specs)?;
        let expanded_disallowed = expand_package_version_specs(disallowed_specs)?;
        Ok(Self::new_with_dep_paths(
            expanded_allowed,
            expanded_disallowed,
            allowed_dep_paths,
            disallowed_dep_paths,
            config.dangerously_allow_all_builds,
        )
        .with_git_repo_rules(allowed_git_repos, disallowed_git_repos))
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

    /// Check whether a package is allowed to run build scripts.
    #[must_use]
    pub fn check(&self, dep_path: &str) -> Option<bool> {
        if self.dangerously_allow_all {
            return Some(true);
        }

        let normalized_dep_path = normalize_build_dep_path(dep_path);
        if self.disallowed_dep_paths.contains(&normalized_dep_path) {
            return Some(false);
        }
        let git_repo_key = git_repo_allow_build_key_from_dep_path(&normalized_dep_path);
        let git_repo_key = git_repo_key.as_deref();
        if let Some(git_repo_key) = git_repo_key
            && self.disallowed_git_repos.contains(git_repo_key)
        {
            return Some(false);
        }
        let (name, version) = parse_name_version_from_key(&normalized_dep_path);
        let name_at_version = format!("{name}@{version}");
        if self.expanded_disallowed.contains(&name)
            || self.expanded_disallowed.contains(&name_at_version)
        {
            return Some(false);
        }
        if self.allowed_dep_paths.contains(&normalized_dep_path) {
            return Some(true);
        }
        if let Some(git_repo_key) = git_repo_key
            && self.allowed_git_repos.contains(git_repo_key)
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
}

/// Strips the peer suffix (and, matching [`PkgVerPeer::without_peer`]'s
/// lumped suffix handling, the patch hash) so config keys compare equal
/// to the `metadata_key.to_string()` form used at the runtime call sites.
///
/// [`PkgVerPeer::without_peer`]: pnpm_lockfile::PkgVerPeer::without_peer
#[must_use]
pub fn normalize_build_dep_path(dep_path: &str) -> String {
    remove_suffix(dep_path).to_string()
}

/// The `allowBuilds` key under which an ignored build should be approved:
/// the package name for registry packages, the peer-suffix-free depPath for
/// git/tarball artifacts (whose name alone must not approve builds).
#[must_use]
pub fn allow_build_key_from_ignored_build(dep_path: &str) -> String {
    let pkg_id_with_patch_hash = get_pkg_id_with_patch_hash(dep_path);
    match parse_dep_path_name_version(pkg_id_with_patch_hash) {
        Some((name, version)) if node_semver::Version::parse(version).is_ok() => name.to_string(),
        _ => pkg_id_with_patch_hash.to_string(),
    }
}

/// Split a peer-suffix-free depPath / pkgId into its `name` and `version`
/// (with any `(patch_hash=…)` segment stripped) — the half of depPath
/// parsing that [`allow_build_key_from_ignored_build`] consumes. Returns
/// `None` when there is no `@` version separator past position 0 or the
/// version is empty — the cases that yield a name-less result.
pub(crate) fn parse_dep_path_name_version(pkg_id: &str) -> Option<(&str, &str)> {
    let sep = pkg_id.get(1..)?.find('@').map(|off| off + 1)?;
    let name = &pkg_id[..sep];
    let mut version = &pkg_id[sep + 1..];
    if version.is_empty() {
        return None;
    }
    let suffix = index_of_dep_path_suffix(version);
    if let Some(idx) = suffix.patch_hash_index {
        version = &version[..idx];
    } else if let Some(idx) = suffix.peers_index {
        version = &version[..idx];
    }
    Some((name, version))
}

pub(crate) fn is_git_repo_allow_build_key(spec: &str) -> bool {
    !spec.contains('#') && is_git_repo_dep_path(spec)
}

pub(crate) fn git_repo_allow_build_key_from_dep_path(dep_path: &str) -> Option<Cow<'_, str>> {
    if is_git_repo_dep_path(dep_path) {
        return Some(match dep_path.find('#') {
            Some(ref_start) => Cow::Borrowed(&dep_path[..ref_start]),
            None => Cow::Borrowed(dep_path),
        });
    }
    // Packages installed from a git host as a downloaded tarball (e.g. the
    // `github:` shortcut, which pnpm fetches from codeload.github.com rather
    // than cloning) have a depPath built from the tarball URL, not a `git+`
    // clone URL, so the check above misses them. Normalize the tarball URL back
    // to the same `git+https://<host>/<repo>.git` repo key that a clone of the
    // same repository would produce, so a single hashless `allowBuilds` entry
    // approves the package however pnpm happened to fetch it.
    git_hosted_tarball_repo_key(dep_path).map(Cow::Owned)
}

pub(crate) fn is_git_repo_dep_path(dep_path: &str) -> bool {
    dep_path.starts_with("git+") || dep_path.contains("@git+")
}

/// Rebuilds the `<name>@git+https://<host>/<repo>.git` repo key for a git-host
/// tarball depPath (mirrors the TypeScript `gitHostedTarballRepoKey`).
pub(crate) fn git_hosted_tarball_repo_key(dep_path: &str) -> Option<String> {
    let (name, version) = parse_dep_path_name_version(dep_path)?;
    let repo_url = git_hosted_tarball_repo_url(version)?;
    Some(format!("{name}@{repo_url}"))
}

/// The committish-free repository URL for a git host that pnpm downloads as a
/// tarball instead of cloning. The patterns mirror the tarball templates in
/// `@pnpm/git-resolver` (from hosted-git-info, except GitLab's override). Each
/// known host is anchored so a look-alike download host (e.g.
/// `codeload.github.com.example.com`) cannot be rewritten into an unrelated key.
pub(crate) fn git_hosted_tarball_repo_url(tarball_url: &str) -> Option<String> {
    // GitHub: `https://codeload.github.com/<owner>/<repo>/tar.gz/<committish>`
    if let Some(rest) = tarball_url.strip_prefix("https://codeload.github.com/") {
        let (owner, rest) = rest.split_once('/')?;
        let (repo, _) = rest.split_once("/tar.gz/")?;
        // `owner` and `repo` are each a single path segment, matching the
        // `[^/]+` anchors in the TypeScript matcher so both stacks normalize
        // identically. `owner` cannot contain a slash (it is the first
        // `split_once('/')` half), but `repo` can, so reject that here.
        if owner.is_empty() || repo.is_empty() || repo.contains('/') {
            return None;
        }
        return Some(format!("git+https://github.com/{owner}/{repo}.git"));
    }
    // Bitbucket: `https://bitbucket.org/<owner>/<repo>/get/<committish>.tar.gz`
    if let Some(rest) = tarball_url.strip_prefix("https://bitbucket.org/") {
        let (owner, rest) = rest.split_once('/')?;
        let (repo, _) = rest.split_once("/get/")?;
        // Single-segment `repo`, as in the GitHub branch above.
        if owner.is_empty() || repo.is_empty() || repo.contains('/') {
            return None;
        }
        return Some(format!("git+https://bitbucket.org/{owner}/{repo}.git"));
    }
    // GitLab (incl. self-hosted): the project path may contain nested groups,
    // so match up to the `/-/archive/<ref>/` marker.
    // `https://<host>/<group...>/<repo>/-/archive/<ref>/<repo>-<ref>.tar.gz`
    if let Some(rest) = tarball_url.strip_prefix("https://") {
        let (host, path) = rest.split_once('/')?;
        if host.is_empty() {
            return None;
        }
        // Take the shortest non-empty project whose marker is followed by a
        // non-empty `<ref>/` segment, mirroring the lazy `(.+?)` project
        // capture and the `[^/]+/` ref anchor of the TypeScript matcher.
        const ARCHIVE_MARKER: &str = "/-/archive/";
        for (marker_index, _) in path.match_indices(ARCHIVE_MARKER) {
            let project = &path[..marker_index];
            let after_marker = &path[marker_index + ARCHIVE_MARKER.len()..];
            let Some((git_ref, _)) = after_marker.split_once('/') else {
                continue;
            };
            if project.is_empty() || git_ref.is_empty() {
                continue;
            }
            return Some(format!("git+https://{host}/{project}.git"));
        }
    }
    None
}

pub(crate) fn is_dep_path_allow_build_key(spec: &str) -> bool {
    if normalize_build_dep_path(spec) != spec {
        return true;
    }
    if spec.contains("||") {
        return false;
    }
    let (_, version) = parse_name_version_from_key(spec);
    if version.is_empty() {
        return !spec.starts_with('@') && (spec.contains('/') || spec.contains(':'));
    }
    node_semver::Version::parse(&version).is_err() && is_source_like_dep_path_version(&version)
}

pub(crate) fn is_source_like_dep_path_version(version: &str) -> bool {
    version.contains(':') || version.contains('/') || version.contains('#')
}
