use super::{
    CanonicalPackageName,
    TarballRevision,
};

pub(super) fn is_addressable_version(version: &str) -> bool {
    pnpr_package_name::is_safe_path_segment(version)
        && !version
            .split_once('+')
            .and_then(|(_, build)| build.strip_prefix('r'))
            .is_some_and(|digits| {
                !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
            })
}

/// The `<version>` segment when `url` ends on `{pkg}@{version}`, with any
/// `?query`/`#fragment` stripped, when `<version>` is a safe non-revision
/// version segment.
#[must_use]
pub fn version_route_tarball_version<'a>(
    url: &'a str,
    pkg: &CanonicalPackageName,
) -> Option<&'a str> {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url);
    let (prefix, version) = path.rsplit_once('@')?;
    ((prefix.ends_with(&format!("/{}", pkg.as_str())) || prefix == pkg.as_str())
        && is_addressable_version(version))
    .then_some(version)
}

/// The `<version>+r<revision>` segment when `url` is a clean revision route
/// ending on `{pkg}@{version}+r<revision>`.
#[must_use]
pub fn revision_route_tarball_version<'a>(
    url: &'a str,
    pkg: &CanonicalPackageName,
) -> Option<&'a str> {
    if url.contains(['?', '#', '\\', '%']) || has_dot_path_segment(url) {
        return None;
    }
    let (prefix, version_and_rev) = url.rsplit_once('@')?;
    if !(prefix.ends_with(&format!("/{}", pkg.as_str())) || prefix == pkg.as_str()) {
        return None;
    }
    let (base_version, build) = version_and_rev.split_once('+')?;
    if !pnpr_package_name::is_safe_path_segment(base_version) || base_version.contains(['+', '@']) {
        return None;
    }
    let digits = build.strip_prefix('r')?;
    let canonical_digits = digits == "0"
        || (!digits.is_empty()
            && !digits.starts_with('0')
            && digits.bytes().all(|byte| byte.is_ascii_digit()));
    let valid_revision = canonical_digits
        && digits
            .parse::<u64>()
            .ok()
            .and_then(|num| TarballRevision::try_from(num).ok())
            .is_some();
    valid_revision.then_some(version_and_rev)
}

fn has_dot_path_segment(url: &str) -> bool {
    let path = match url
        .split_once("://")
        .or_else(|| {
            url.strip_prefix("//")
                .map(|rest| ("", rest))
        }) {
        Some((_, after_authority_start)) => after_authority_start
            .split_once('/')
            .map(|(_, path)| path)
            .unwrap_or(""),
        None => url,
    };
    path.split('/')
        .any(|seg| seg == "." || seg == "..")
}

/// The tarball filename a `dist.tarball` URL addresses for `pkg`: the canonical
/// `<basename>-<version>.tgz` when `url` is a `{pkg}@{version}` route, or
/// [`tarball_basename`] otherwise.
#[must_use]
pub fn tarball_basename_for_package<'a>(
    url: &'a str,
    pkg: &CanonicalPackageName,
) -> Option<std::borrow::Cow<'a, str>> {
    if let Some(version) = version_route_tarball_version(url, pkg) {
        return Some(std::borrow::Cow::Owned(pkg.tarball_name_for_version(version)));
    }
    tarball_basename(url).map(std::borrow::Cow::Borrowed)
}

/// The tarball filename a `dist.tarball` URL points at: the final path
/// segment, with any `?query`/`#fragment` stripped. This basename is the
/// trust key shared by the rewritten public URL ([`super::rewrite_tarball_urls`])
/// and the serve-time version match (`expected_tarball_dist`), so both
/// must derive it identically — including for query-bearing URLs (signed
/// CDN links), where the query is not part of the route path a client
/// later requests. Returns `None` for a URL whose path ends in `/`.
#[must_use]
pub fn tarball_basename(url: &str) -> Option<&str> {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url);
    path.rsplit('/')
        .next()
        .filter(|segment| !segment.is_empty())
}
