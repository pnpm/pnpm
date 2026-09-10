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
        let mut allowed = BuildKeys::default();
        let mut disallowed = BuildKeys::default();
        for (spec, &value) in &config.allow_builds {
            let keys = if value { &mut allowed } else { &mut disallowed };
            keys.add(spec);
        }
        Ok(Self::new_with_dep_paths(
            expand_package_version_specs(allowed.specs)?,
            expand_package_version_specs(disallowed.specs)?,
            allowed.dep_paths,
            disallowed.dep_paths,
            config.dangerously_allow_all_builds,
        )
        .with_git_repo_rules(allowed.git_repos, disallowed.git_repos))
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
        let git_repo_key = git_repo_allow_build_key_from_dep_path(&normalized_dep_path);
        let git_repo_key = git_repo_key.as_deref();
        let (name, version) = parse_name_version_from_key(&normalized_dep_path);
        let name_at_version = format!("{name}@{version}");
        if self.denies(&normalized_dep_path, git_repo_key, (&name, &name_at_version)) {
            return Some(false);
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
    // A URL on a claimed download host that does not match that host's tarball
    // pattern is rejected outright — it must not fall through to the generic
    // GitLab matcher and produce the host's trusted repo key.
    //
    // GitHub: `https://codeload.github.com/<owner>/<repo>/tar.gz/<committish>`
    if let Some(rest) = tarball_url.strip_prefix("https://codeload.github.com/") {
        let (owner, repo) = single_segment_owner_repo(rest, "/tar.gz/")?;
        return Some(format!("git+https://github.com/{owner}/{repo}.git"));
    }
    // Bitbucket: `https://bitbucket.org/<owner>/<repo>/get/<committish>.tar.gz`
    if let Some(rest) = tarball_url.strip_prefix("https://bitbucket.org/") {
        let (owner, repo) = single_segment_owner_repo(rest, "/get/")?;
        return Some(format!("git+https://bitbucket.org/{owner}/{repo}.git"));
    }
    gitlab_tarball_repo_url(tarball_url)
}

/// `owner` and `repo` are each a single path segment, matching the `[^/]+`
/// anchors in the TypeScript matcher so both stacks normalize identically.
/// `owner` cannot contain a slash (it is the first `split_once('/')` half),
/// but `repo` can, so that is rejected here.
fn single_segment_owner_repo<'a>(rest: &'a str, marker: &str) -> Option<(&'a str, &'a str)> {
    let (owner, rest) = rest.split_once('/')?;
    let (repo, _) = rest.split_once(marker)?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    Some((owner, repo))
}

/// GitLab (incl. self-hosted): the project path may contain nested groups, so
/// the match runs up to the `/-/archive/<ref>/` marker.
/// `https://<host>/<group...>/<repo>/-/archive/<ref>/<repo>-<ref>.tar.gz`
fn gitlab_tarball_repo_url(tarball_url: &str) -> Option<String> {
    let rest = tarball_url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    if host.is_empty() {
        return None;
    }
    let project = gitlab_archive_project(path)?;
    Some(format!("git+https://{host}/{project}.git"))
}

/// The shortest non-empty project whose marker is followed by a non-empty
/// `<ref>/` segment, mirroring the lazy `(.+?)` project capture and the
/// `[^/]+/` ref anchor of the TypeScript matcher.
fn gitlab_archive_project(path: &str) -> Option<&str> {
    const ARCHIVE_MARKER: &str = "/-/archive/";
    path.match_indices(ARCHIVE_MARKER).find_map(|(marker_index, _)| {
        let project = &path[..marker_index];
        let after_marker = &path[marker_index + ARCHIVE_MARKER.len()..];
        let (git_ref, _) = after_marker.split_once('/')?;
        (!project.is_empty() && !git_ref.is_empty()).then_some(project)
    })
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
