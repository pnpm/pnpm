/// Returns whether both inputs identify the same supported Git repository and ref.
///
/// Hosted shortcuts and bare GitHub shortcuts compare with `git://`,
/// `git+https://`, and hosted `https://` forms after adding or removing the
/// conventional `.git` suffix. Authentication-bearing, query-bearing, SSH,
/// and plain HTTP specifiers remain distinct.
#[must_use]
pub fn git_specifiers_are_equivalent(left: &str, right: &str) -> bool {
    let Some(left) = normalize_git_specifier(left) else {
        return false;
    };
    let Some(right) = normalize_git_specifier(right) else {
        return false;
    };
    left == right
}

fn normalize_git_specifier(specifier: &str) -> Option<String> {
    normalize_shortcut(specifier).or_else(|| normalize_url(specifier))
}

fn normalize_shortcut(specifier: &str) -> Option<String> {
    let (repository, committish) = split_committish(specifier)?;
    let (host, path) = if let Some(path) = repository.strip_prefix("github:") {
        ("github.com", path)
    } else if let Some(path) = repository.strip_prefix("gitlab:") {
        ("gitlab.com", path)
    } else if let Some(path) = repository.strip_prefix("bitbucket:") {
        ("bitbucket.org", path)
    } else if is_github_shorthand(repository) {
        ("github.com", repository)
    } else {
        return None;
    };
    normalize_parts(host, path, committish)
}

fn normalize_url(specifier: &str) -> Option<String> {
    let (repository, committish) = split_committish(specifier)?;
    let (scheme, location) = repository.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("git")
        && !scheme.eq_ignore_ascii_case("git+https")
        && !scheme.eq_ignore_ascii_case("https")
    {
        return None;
    }
    let (host, path) = location.split_once('/')?;
    if host.is_empty()
        || host.contains('@')
        || host.contains('?')
        || host.chars().any(char::is_whitespace)
    {
        return None;
    }
    if scheme.eq_ignore_ascii_case("https") && !is_known_host(host) && !path.ends_with(".git") {
        return None;
    }
    normalize_parts(host, path, committish)
}

fn normalize_parts(host: &str, path: &str, committish: Option<&str>) -> Option<String> {
    if path.is_empty()
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains("//")
        || path.contains('@')
        || path.contains('?')
        || path.chars().any(char::is_whitespace)
        || path.split('/').any(str::is_empty)
    {
        return None;
    }
    let path = path.strip_suffix(".git").unwrap_or(path);
    if path.is_empty() || path.ends_with('/') {
        return None;
    }
    match committish.filter(|committish| !committish.is_empty()) {
        Some(committish) => Some(format!("git+https://{host}/{path}.git#{committish}")),
        None => Some(format!("git+https://{host}/{path}.git")),
    }
}

fn split_committish(specifier: &str) -> Option<(&str, Option<&str>)> {
    let Some((repository, committish)) = specifier.split_once('#') else {
        return Some((specifier, None));
    };
    (!committish.contains('#')).then_some((repository, Some(committish)))
}

fn is_github_shorthand(repository: &str) -> bool {
    !repository.starts_with('.')
        && !repository.chars().any(char::is_whitespace)
        && !repository.contains([':', '@'])
        && repository.split('/').count() == 2
}

fn is_known_host(host: &str) -> bool {
    matches!(host, "github.com" | "gitlab.com" | "bitbucket.org")
}

#[cfg(test)]
mod tests;
