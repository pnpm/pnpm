use super::{Cow, TarballRevision};

/// Build the registry-relative tarball path `<name>@<version>+r<revision>`.
#[must_use]
pub fn revision_registry_tarball_path(
    name: &str,
    version: &str,
    revision: TarballRevision,
) -> Option<String> {
    if !is_valid_revision_package_name(name) || !is_valid_revision_version(version) {
        return None;
    }
    Some(format!("{name}@{version}+r{revision}"))
}

/// Build a revision tarball URL `<registry>/<name>@<version>+r<revision>` relative to `registry`.
#[must_use]
pub fn revision_registry_tarball_url(
    name: &str,
    version: &str,
    revision: TarballRevision,
    registry: &str,
) -> Option<String> {
    let path = revision_registry_tarball_path(name, version, revision)?;
    let base = parse_clean_registry_base(registry)?;
    base.join(&path).ok().map(Into::into)
}

/// Whether `tarball` is the exact `<registry>/<name>@<version>+r<revision>` URL.
#[must_use]
pub fn is_revision_registry_tarball_url(
    tarball: &str,
    name: &str,
    version: &str,
    revision: TarballRevision,
    registry: &str,
) -> bool {
    if has_raw_path_traversal(tarball) {
        return false;
    }
    let Some(expected) = revision_registry_tarball_url(name, version, revision, registry) else {
        return false;
    };
    match (parse_clean_url(tarball), url::Url::parse(&expected)) {
        (Some(actual), Ok(expected)) => actual == expected,
        _ => false,
    }
}

/// Whether `tarball` sits on `registry`'s `<name>@<version>+r<digits>` revision route.
#[must_use]
pub fn is_on_revision_tarball_route(tarball: &str, registry: &str) -> bool {
    let Some(path) = registry_relative_path(tarball, registry) else {
        return false;
    };
    let Some(at) = path.rfind('@').filter(|at| *at > 0) else {
        return false;
    };
    let (name, version_and_rev) = (&path[..at], &path[at + 1..]);
    if !is_valid_revision_package_name(name) {
        return false;
    }
    let Some((version, build)) = version_and_rev.split_once('+') else {
        return false;
    };
    is_valid_revision_version(version)
        && build
            .strip_prefix('r')
            .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

fn registry_relative_path(tarball: &str, registry: &str) -> Option<String> {
    let actual = parse_clean_url(tarball)?;
    let base = parse_clean_registry_base(registry)?;
    if actual.origin() != base.origin() {
        return None;
    }
    actual
        .path()
        .strip_prefix(base.path())
        .map(str::to_string)
}

fn parse_clean_registry_base(registry: &str) -> Option<url::Url> {
    let registry_with_slash = if registry.ends_with('/') {
        Cow::Borrowed(registry)
    } else {
        Cow::Owned(format!("{registry}/"))
    };
    parse_clean_url(&registry_with_slash)
}

fn parse_clean_url(raw: &str) -> Option<url::Url> {
    if has_raw_path_traversal(raw) {
        return None;
    }
    let parsed = url::Url::parse(raw).ok()?;
    if parsed.query().is_some()
        || parsed.fragment().is_some()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    Some(parsed)
}

fn is_valid_revision_package_name(name: &str) -> bool {
    if name.is_empty() || !is_clean_url_segment(name) {
        return false;
    }
    if let Some(scoped) = name.strip_prefix('@') {
        let Some((scope, pkg)) = scoped.split_once('/') else {
            return false;
        };
        !scope.is_empty()
            && !pkg.is_empty()
            && !pkg.contains('/')
            && !scope.contains('@')
            && !pkg.contains('@')
            && scope != "."
            && scope != ".."
            && pkg != "."
            && pkg != ".."
    } else {
        !name.contains('/') && !name.contains('@') && name != "." && name != ".."
    }
}

fn is_valid_revision_version(version: &str) -> bool {
    !version.is_empty()
        && version != "."
        && version != ".."
        && !version.contains('/')
        && !version.contains('+')
        && !version.contains('@')
        && is_clean_url_segment(version)
}

fn is_clean_url_segment(segment: &str) -> bool {
    !segment.contains('\\')
        && !segment.contains('?')
        && !segment.contains('#')
        && !segment.contains('%')
        && !segment
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
}

fn has_raw_path_traversal(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if lower.contains('\\')
        || lower.contains("%2e")
        || lower.contains("%2f")
        || lower.contains("%5c")
    {
        return true;
    }
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let path = after_scheme
        .split_once('/')
        .map_or("", |(_, path)| path)
        .split(['?', '#'])
        .next()
        .unwrap_or("");
    path.split('/')
        .any(|seg| seg == "." || seg == "..")
}
