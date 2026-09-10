/// Recognizes immutable archive URLs emitted by known git providers. The result
/// gates integrity exemptions, so path shapes are matched explicitly and refs
/// must be full commit SHAs.
#[must_use]
pub fn is_git_hosted_tarball_url(url: &str) -> bool {
    let Some((host, path, query)) = parse_https_url(url) else { return false };
    if host.eq_ignore_ascii_case("codeload.github.com") {
        return is_github_codeload_archive(path);
    }
    if host.eq_ignore_ascii_case("bitbucket.org") {
        return is_bitbucket_archive(path);
    }
    if host.eq_ignore_ascii_case("gitlab.com") {
        return is_gitlab_archive(path, query);
    }
    false
}

fn parse_https_url(url: &str) -> Option<(&str, &str, Option<&str>)> {
    const HTTPS_SCHEME: &str = "https://";
    if !url.get(..HTTPS_SCHEME.len())?.eq_ignore_ascii_case(HTTPS_SCHEME) {
        return None;
    }
    let rest = url.get(HTTPS_SCHEME.len()..)?;
    let (host, path_and_query) = rest.split_once('/')?;
    let path_and_query = path_and_query.split_once('#').map_or(path_and_query, |(path, _)| path);
    let (path, query) = path_and_query
        .split_once('?')
        .map_or((path_and_query, None), |(path, query)| (path, Some(query)));
    Some((host, path, query))
}

fn is_github_codeload_archive(path: &str) -> bool {
    let segments = path_segments(path);
    segments.len() == 4 && segments[2] == "tar.gz" && is_full_commit_sha(segments[3])
}

fn is_bitbucket_archive(path: &str) -> bool {
    let segments = path_segments(path);
    if segments.len() != 4 || segments[2] != "get" {
        return false;
    }
    let Some(commit) = segments[3].strip_suffix(".tar.gz") else { return false };
    is_full_commit_sha(commit)
}

fn is_gitlab_archive(path: &str, query: Option<&str>) -> bool {
    let segments = path_segments(path);
    if segments.len() == 6
        && segments[0] == "api"
        && segments[1] == "v4"
        && segments[2] == "projects"
        && segments[4] == "repository"
        && segments[5] == "archive.tar.gz"
    {
        return query_param(query, "ref").is_some_and(is_full_commit_sha);
    }
    let Some(archive_marker_index) =
        segments.windows(2).position(|window| window[0] == "-" && window[1] == "archive")
    else {
        return false;
    };
    if archive_marker_index < 2 || segments.len() != archive_marker_index + 4 {
        return false;
    }
    let commit = segments[archive_marker_index + 2];
    let archive_name = segments[archive_marker_index + 3];
    archive_name.ends_with(".tar.gz") && is_full_commit_sha(commit)
}

fn path_segments(path: &str) -> Vec<&str> {
    path.split('/').filter(|segment| !segment.is_empty()).collect()
}

fn query_param<'query>(query: Option<&'query str>, key: &str) -> Option<&'query str> {
    query?.split('&').find_map(|part| {
        let (part_key, value) = part.split_once('=')?;
        (part_key == key).then_some(value)
    })
}

fn is_full_commit_sha(value: &str) -> bool {
    value.len() == 40 && value.as_bytes().iter().all(u8::is_ascii_hexdigit)
}
