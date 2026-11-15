use super::{ActionReference, HashMap, RepoVersion, Version};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CommentVersion {
    Exact(Version),
    MajorMinor(u64, u64),
    Major(u64),
}

pub(super) fn repo_versions(refs: &HashMap<String, String>) -> Vec<RepoVersion> {
    let mut versions = refs
        .iter()
        .filter_map(|(ref_, commit)| {
            let tag = ref_.strip_prefix("refs/tags/")?;
            if tag.ends_with("^{}") {
                return None;
            }
            let version = parse_version(tag)?;
            Some(RepoVersion {
                commit: refs
                    .get(&format!("{ref_}^{{}}"))
                    .unwrap_or(commit)
                    .clone(),
                tag: tag.to_string(),
                version,
            })
        })
        .collect::<Vec<_>>();
    versions.sort_by(|left, right| left.version.cmp(&right.version));
    versions
}

pub(super) fn find_current(
    action: &ActionReference,
    versions: &[RepoVersion],
) -> Option<RepoVersion> {
    if is_sha(&action.ref_)
        && let Some(comment) = &action.source.comment_version
        && let Some(parsed) = parse_comment_version(comment)
        && let Some(current) = find_matching_commit_version(&action.ref_, &parsed, versions)
    {
        return Some(current);
    }
    if !is_sha(&action.ref_)
        && let Some(parsed) = parse_comment_version(&action.ref_)
    {
        return find_matching_ref_version(&parsed, versions);
    }
    if is_sha(&action.ref_) {
        return versions
            .iter()
            .rfind(|candidate| candidate.commit == action.ref_)
            .cloned();
    }
    None
}

fn find_matching_commit_version(
    commit: &str,
    comment: &CommentVersion,
    versions: &[RepoVersion],
) -> Option<RepoVersion> {
    match comment {
        CommentVersion::Exact(version) => versions
            .iter()
            .find(|candidate| candidate.commit == commit && &candidate.version == version)
            .cloned(),
        CommentVersion::MajorMinor(major, minor) => versions
            .iter()
            .rfind(|candidate| {
                candidate.commit == commit
                    && candidate.version.major == *major
                    && candidate.version.minor == *minor
                    && candidate.version.pre_release.is_empty()
            })
            .cloned(),
        CommentVersion::Major(major) => versions
            .iter()
            .rfind(|candidate| {
                candidate.commit == commit
                    && candidate.version.major == *major
                    && candidate.version.pre_release.is_empty()
            })
            .cloned(),
    }
}

fn find_matching_ref_version(
    target: &CommentVersion,
    versions: &[RepoVersion],
) -> Option<RepoVersion> {
    match target {
        CommentVersion::Exact(version) => versions
            .iter()
            .find(|candidate| &candidate.version == version)
            .cloned(),
        CommentVersion::MajorMinor(major, minor) => versions
            .iter()
            .rfind(|candidate| {
                candidate.version.major == *major
                    && candidate.version.minor == *minor
                    && candidate.version.pre_release.is_empty()
            })
            .cloned(),
        CommentVersion::Major(major) => versions
            .iter()
            .rfind(|candidate| {
                candidate.version.major == *major
                    && candidate.version.pre_release.is_empty()
            })
            .cloned(),
    }
}

fn is_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn replaces_comment_version(token: &str, current: &Version) -> bool {
    let Some(parsed) = parse_comment_version(token) else {
        return false;
    };
    if token.starts_with('v') {
        return true;
    }
    match parsed {
        CommentVersion::Exact(_) => true,
        CommentVersion::MajorMinor(major, minor) => {
            current.major == major && current.minor == minor
        }
        CommentVersion::Major(major) => current.major == major,
    }
}

pub(super) fn parse_comment_version(input: &str) -> Option<CommentVersion> {
    if let Some(version) = parse_version(input) {
        return Some(CommentVersion::Exact(version));
    }
    let rest = input.strip_prefix('v').unwrap_or(input);
    if let Some((major, minor)) = rest.split_once('.') {
        return Some(CommentVersion::MajorMinor(
            parse_version_number(major)?,
            parse_version_number(minor)?,
        ));
    }
    parse_version_number(rest).map(CommentVersion::Major)
}

fn parse_version_number(segment: &str) -> Option<u64> {
    (!segment.is_empty() && segment.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| segment.parse().ok())
        .flatten()
}

fn parse_version(input: &str) -> Option<Version> {
    Version::parse(input)
        .or_else(|_| Version::parse(input.trim_start_matches('v')))
        .ok()
}
