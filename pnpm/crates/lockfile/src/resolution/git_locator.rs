use super::LockfileResolution;

impl LockfileResolution {
    /// Locator used as the package ID for a Git checkout or a Git host's archive.
    #[must_use]
    pub fn git_locator(&self) -> Option<String> {
        match self {
            Self::Tarball(tarball) if tarball.git_hosted == Some(true) => {
                Some(match &tarball.path {
                    Some(path) => format!("{}#path:{path}", tarball.tarball),
                    None => tarball.tarball.clone(),
                })
            }
            Self::Git(git) => {
                Some(create_git_hosted_pkg_id(&git.repo, &git.commit, git.path.as_deref()))
            }
            _ => None,
        }
    }
}

/// Build the URL-shaped ID for a `Git` lockfile resolution.
///
/// The output is the `PkgResolutionId` stamped as `id` on a git
/// `ResolveResult`.
#[must_use]
pub fn create_git_hosted_pkg_id(repo: &str, commit: &str, path: Option<&str>) -> String {
    let mut id = if repo.contains("://") {
        format!("{repo}#{commit}")
    } else {
        format!("https://{repo}#{commit}")
    };
    if !id.starts_with("git+") {
        id.insert_str(0, "git+");
    }
    if let Some(path) = path {
        id.push_str("&path:");
        id.push_str(path);
    }
    id
}

#[cfg(test)]
mod tests;
