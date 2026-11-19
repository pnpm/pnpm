use pnpm_deps_path::{
    get_pkg_id_with_patch_hash, index_of_dep_path_suffix, parse_registry_qualified_version,
    version_separator_index,
};
use std::borrow::Cow;

/// A short display label for a dependency path or approval key.
#[must_use]
pub fn short_package_label(key: &str) -> String {
    let package_id = if is_link_dep_path(key) { key } else { get_pkg_id_with_patch_hash(key) };
    let (name, source) = match parse_dep_path_name_version(package_id) {
        Some((name, version))
            if node_semver::Version::parse(version).is_ok()
                || parse_registry_qualified_version(version).is_some() =>
        {
            (name, None)
        }
        Some((name, version)) => (name, Some(source_tag(version))),
        None => (package_id, None),
    };
    if is_git_hosted_dep_path(package_id) {
        return format!("{name} (git)");
    }
    match source {
        Some(tag) => format!("{name} ({tag})"),
        None => name.to_string(),
    }
}

/// Split a dependency path or package ID into its package name and version.
#[must_use]
pub fn parse_dep_path_name_version(package_id: &str) -> Option<(&str, &str)> {
    let separator = version_separator_index(package_id)?;
    let name = &package_id[..separator];
    if name.contains(':') {
        return None;
    }
    let mut version = &package_id[separator + 1..];
    if version.is_empty() {
        return None;
    }
    if version.starts_with("link:") {
        return Some((name, version));
    }
    let suffix = index_of_dep_path_suffix(version);
    if let Some(index) = suffix.patch_hash_index {
        version = &version[..index];
    } else if let Some(index) = suffix.peers_index {
        version = &version[..index];
    }
    Some((name, version))
}

/// Whether a dependency path or package ID names a package from a git host.
#[must_use]
pub fn is_git_hosted_dep_path(dep_path: &str) -> bool {
    git_repo_allow_build_key_from_dep_path(dep_path).is_some()
}

/// The repository key represented by a git dependency path.
#[must_use]
pub fn git_repo_allow_build_key_from_dep_path(dep_path: &str) -> Option<Cow<'_, str>> {
    if is_git_repo_dep_path(dep_path) {
        return Some(match dep_path.find('#') {
            Some(ref_start) => Cow::Borrowed(&dep_path[..ref_start]),
            None => Cow::Borrowed(dep_path),
        });
    }
    git_hosted_tarball_repo_key(dep_path).map(Cow::Owned)
}

/// Whether a package ID contains a git repository specifier.
#[must_use]
pub fn is_git_repo_dep_path(dep_path: &str) -> bool {
    dep_path.starts_with("git+") || dep_path.contains("@git+")
}

/// Whether a version component resolves to a local directory.
#[must_use]
pub fn is_local_directory_version(version: &str) -> bool {
    if version.starts_with("link:") {
        return true;
    }
    if let Some(path) = version.strip_prefix("file:") {
        return !is_tarball_path(path);
    }
    false
}

fn is_link_dep_path(dep_path: &str) -> bool {
    parse_dep_path_name_version(dep_path).is_some_and(|(_, version)| version.starts_with("link:"))
}

fn source_tag(version: &str) -> &'static str {
    if is_local_directory_version(version) { "local" } else { "tarball" }
}

fn is_tarball_path(path: &str) -> bool {
    [".tgz", ".tar.gz", ".tar"]
        .iter()
        .any(|suffix| path.to_ascii_lowercase().ends_with(suffix))
}

fn git_hosted_tarball_repo_key(dep_path: &str) -> Option<String> {
    let (name, version) = parse_dep_path_name_version(dep_path)?;
    let repo_url = git_hosted_tarball_repo_url(version)?;
    Some(format!("{name}@{repo_url}"))
}

fn git_hosted_tarball_repo_url(tarball_url: &str) -> Option<String> {
    if let Some(rest) = tarball_url.strip_prefix("https://codeload.github.com/") {
        let (owner, repo) = single_segment_owner_repo(rest, "/tar.gz/")?;
        return Some(format!("git+https://github.com/{owner}/{repo}.git"));
    }
    if let Some(rest) = tarball_url.strip_prefix("https://bitbucket.org/") {
        let (owner, repo) = single_segment_owner_repo(rest, "/get/")?;
        return Some(format!("git+https://bitbucket.org/{owner}/{repo}.git"));
    }
    gitlab_tarball_repo_url(tarball_url)
}

fn single_segment_owner_repo<'a>(rest: &'a str, marker: &str) -> Option<(&'a str, &'a str)> {
    let (owner, rest) = rest.split_once('/')?;
    let (repo, _) = rest.split_once(marker)?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    Some((owner, repo))
}

fn gitlab_tarball_repo_url(tarball_url: &str) -> Option<String> {
    let rest = tarball_url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    if host.is_empty() {
        return None;
    }
    let project = gitlab_archive_project(path)?;
    Some(format!("git+https://{host}/{project}.git"))
}

fn gitlab_archive_project(path: &str) -> Option<&str> {
    const ARCHIVE_MARKER: &str = "/-/archive/";
    path.match_indices(ARCHIVE_MARKER)
        .find_map(|(marker_index, _)| {
            let project = &path[..marker_index];
            let after_marker = &path[marker_index + ARCHIVE_MARKER.len()..];
            let (git_ref, _) = after_marker.split_once('/')?;
            (!project.is_empty() && !git_ref.is_empty()).then_some(project)
        })
}

#[cfg(test)]
mod tests;
