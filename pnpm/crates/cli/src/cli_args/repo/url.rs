use super::{Cow, HostedRepo};

pub(super) fn repository_to_web_url(raw_url: &str, directory: Option<&str>) -> Option<String> {
    if raw_url.is_empty() {
        return None;
    }

    if let Some(url) = try_hosted_shorthand(raw_url, directory) {
        return Some(url);
    }

    let input = raw_url.strip_prefix("git+").unwrap_or(raw_url);
    let cleaned = if let Some(rest) = input.strip_prefix("git://") {
        Cow::Owned(format!("https://{rest}"))
    } else {
        Cow::Borrowed(input)
    };

    let mut parsed = url::Url::parse(&cleaned).ok()?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return None;
    }

    let fragment = try_extract_fragment(raw_url);
    parsed.set_fragment(None);
    parsed.set_query(None);

    let mut base_url = parsed.to_string();
    if base_url.ends_with('/') {
        base_url.pop();
    }
    if base_url.ends_with(".git") {
        base_url.truncate(base_url.len() - 4);
    }
    Some(browse_url(base_url, directory, fragment.as_deref(), "HEAD"))
}

/// The URL a browser opens: the repository itself, or the directory /
/// branch within it that the manifest names.
pub(super) fn browse_url(
    base_url: String,
    directory: Option<&str>,
    fragment: Option<&str>,
    default_branch: &str,
) -> String {
    if let Some(dir) = directory {
        let branch = fragment.unwrap_or(default_branch);
        return format!("{base_url}/tree/{branch}/{}", dir.trim_start_matches('/'));
    }
    match fragment {
        Some(branch) => format!("{base_url}/tree/{branch}"),
        None => base_url,
    }
}

pub(super) fn try_hosted_shorthand(raw_url: &str, directory: Option<&str>) -> Option<String> {
    let cleaned = raw_url
        .strip_prefix("git+")
        .unwrap_or(raw_url)
        .strip_prefix("git://")
        .unwrap_or(raw_url);

    let Some((hosted, path)) = hosted_shorthand(cleaned) else {
        return try_user_repo_shorthand(raw_url, directory);
    };

    let fragment = try_extract_fragment(raw_url);
    let path_clean = path
        .split(&['#', '?'][..])
        .next()
        .unwrap_or(path)
        .trim_end_matches('/');
    let path_no_git = path_clean.trim_end_matches(".git");
    let parts: Vec<&str> = path_no_git.split('/').collect();
    if parts.len() < 2 {
        return None;
    }

    let hosted_base_url = &hosted.base_url;
    let browse_path = format!("{hosted_base_url}/{}", parts[..2].join("/"));

    Some(if let Some(dir) = directory {
        let branch = fragment.as_deref().unwrap_or(hosted.default_branch);
        format!(
            "{browse_path}/tree/{branch}/{}",
            dir.trim_start_matches('/'),
        )
    } else if let Some(branch) = fragment {
        format!("{browse_path}/tree/{branch}")
    } else {
        browse_path
    })
}

pub(super) fn try_user_repo_shorthand(raw_url: &str, directory: Option<&str>) -> Option<String> {
    let cleaned = raw_url.strip_prefix("git+").unwrap_or(raw_url);

    if cleaned.contains("://") || cleaned.starts_with("git@") {
        return try_hosted_url(raw_url, directory);
    }

    if let Some(rest) = cleaned.strip_prefix("github:") {
        return build_hosted_browse_url("https://github.com", rest, "master", directory);
    }

    if let Some(rest) = cleaned.strip_prefix("gitlab:") {
        return build_hosted_browse_url("https://gitlab.com", rest, "master", directory);
    }

    if let Some(rest) = cleaned.strip_prefix("bitbucket:") {
        return build_hosted_browse_url("https://bitbucket.org", rest, "master", directory);
    }

    let fragment = try_extract_fragment(raw_url);
    let path_clean = cleaned
        .split(&['#', '?'][..])
        .next()
        .unwrap_or(cleaned)
        .trim_end_matches('/');
    let (user, repo) = path_clean.split_once('/')?;
    let repo = repo
        .split('/')
        .next()
        .unwrap_or(repo)
        .trim_end_matches(".git");
    // A dotted first segment is a host, not a GitHub user, so the whole
    // reference is a URL rather than the `user/repo` shorthand.
    if user.contains('.') {
        return try_hosted_url(raw_url, directory);
    }
    Some(browse_url(
        format!("https://github.com/{user}/{repo}"),
        directory,
        fragment.as_deref(),
        "master",
    ))
}

pub(super) fn try_hosted_url(raw_url: &str, directory: Option<&str>) -> Option<String> {
    let (parsed, fragment) = parse_hosted_input(raw_url)?;

    let host = parsed.host_str()?;

    let (base_url, default_branch) = match host {
        "github.com" => ("https://github.com", "master"),
        "gitlab.com" => ("https://gitlab.com", "master"),
        "bitbucket.org" | "bitbucket.com" => ("https://bitbucket.org", "master"),
        _ => return None,
    };

    let path_clean = parsed.path().trim_end_matches('/');
    let repo_path = path_clean
        .strip_prefix('/')
        .unwrap_or(path_clean)
        .trim_end_matches(".git");

    let browse_path = format!("{base_url}/{repo_path}");

    Some(if let Some(dir) = directory {
        let branch = fragment.as_deref().unwrap_or(default_branch);
        format!(
            "{browse_path}/tree/{branch}/{}",
            dir.trim_start_matches('/'),
        )
    } else if let Some(branch) = fragment {
        format!("{browse_path}/tree/{branch}")
    } else {
        browse_path
    })
}

/// The repository as an `https://<host>/<path>` URL plus its `#branch`
/// fragment, from the `git+`, SCP-style SSH or `git://` spelling.
pub(super) fn parse_hosted_input(raw_url: &str) -> Option<(url::Url, Option<String>)> {
    let input = raw_url.strip_prefix("git+").unwrap_or(raw_url);
    if let Some(rest) = input.strip_prefix("git@") {
        // SCP-style SSH: git@<host>:<owner>/<repo>(.git)?(#branch)?
        let (scp_host, scp_path) = rest.split_once(':')?;
        let path_only = scp_path
            .split(&['#', '?'][..])
            .next()
            .unwrap_or(scp_path);
        let parsed = url::Url::parse(&format!("https://{scp_host}/{path_only}")).ok()?;
        return Some((parsed, try_extract_fragment(raw_url)));
    }
    let normalized = if let Some(rest) = input.strip_prefix("git://") {
        Cow::Owned(format!("https://{rest}"))
    } else {
        Cow::Borrowed(input)
    };
    let frag = try_extract_fragment(raw_url);
    let parsed = url::Url::parse(&normalized).ok()?;
    Some((parsed, frag))
}

pub(super) fn build_hosted_browse_url(
    base_url: &str,
    path: &str,
    default_branch: &str,
    directory: Option<&str>,
) -> Option<String> {
    let path_clean = path
        .split(&['#', '?'][..])
        .next()
        .unwrap_or(path)
        .trim_end_matches('/');
    let path_no_git = path_clean.trim_end_matches(".git");
    let parts: Vec<&str> = path_no_git.split('/').collect();
    if parts.len() < 2 {
        return None;
    }

    let browse_path = format!("{base_url}/{}", parts[..2].join("/"));
    let fragment = try_extract_fragment(path);

    Some(if let Some(dir) = directory {
        let branch = fragment.as_deref().unwrap_or(default_branch);
        format!(
            "{browse_path}/tree/{branch}/{}",
            dir.trim_start_matches('/'),
        )
    } else if let Some(branch) = fragment {
        format!("{browse_path}/tree/{branch}")
    } else {
        browse_path
    })
}

pub(super) fn try_extract_fragment(raw_url: &str) -> Option<String> {
    let (_, after_hash) = raw_url.split_once('#')?;
    let fragment = after_hash.split('?').next()?;
    if fragment.is_empty() {
        None
    } else {
        Some(fragment.to_string())
    }
}

fn hosted_shorthand(cleaned: &str) -> Option<(HostedRepo, &str)> {
    let hosted = if let Some(rest) = cleaned.strip_prefix("github:") {
        (
            HostedRepo {
                base_url: "https://github.com".to_string(),
                default_branch: "master",
            },
            rest,
        )
    } else if let Some(rest) = cleaned.strip_prefix("gitlab:") {
        (
            HostedRepo {
                base_url: "https://gitlab.com".to_string(),
                default_branch: "master",
            },
            rest,
        )
    } else {
        let rest = cleaned.strip_prefix("bitbucket:")?;
        (
            HostedRepo {
                base_url: "https://bitbucket.org".to_string(),
                default_branch: "master",
            },
            rest,
        )
    };
    Some(hosted)
}
