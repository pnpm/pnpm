use super::{Identifier, Path, Version, VersionError};

/// A parsed version argument: an exact version to set, or a release type to
/// increment by.
#[derive(Debug)]
pub(super) enum Bump {
    Explicit(Version),
    Release(ReleaseType),
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ReleaseType {
    Major,
    Minor,
    Patch,
    Premajor,
    Preminor,
    Prepatch,
    Prerelease,
}

/// Parse the version argument: a valid semver version wins (like upstream's
/// `semver.valid`, so a leading `v` is accepted and stripped), then the
/// release-type keywords.
pub(super) fn parse_bump(raw: &str) -> Result<Bump, VersionError> {
    if let Ok(version) = Version::parse(raw) {
        return Ok(Bump::Explicit(version));
    }
    let release = match raw {
        "major" => ReleaseType::Major,
        "minor" => ReleaseType::Minor,
        "patch" => ReleaseType::Patch,
        "premajor" => ReleaseType::Premajor,
        "preminor" => ReleaseType::Preminor,
        "prepatch" => ReleaseType::Prepatch,
        "prerelease" => ReleaseType::Prerelease,
        _ => return Err(VersionError::InvalidBump { raw: raw.to_string() }),
    };
    Ok(Bump::Release(release))
}

/// Increment `version` by `release`, following node-semver's `inc()` with its
/// default identifier base of `0`: bumping from a prerelease of the next
/// major/minor/patch merely finalizes it, the `pre*` types start a `.0`
/// prerelease (prefixed with `preid` when given), and `prerelease` increments
/// the right-most numeric identifier.
pub(super) fn inc(version: &Version, release: ReleaseType, preid: Option<&str>) -> Version {
    let mut next = version.clone();
    next.build = Vec::new();
    match release {
        ReleaseType::Major => bump_major(&mut next),
        ReleaseType::Minor => {
            if !releases_pending_minor(&next) {
                next.minor += 1;
            }
            next.patch = 0;
            next.pre_release = Vec::new();
        }
        ReleaseType::Patch => {
            if next.pre_release.is_empty() {
                next.patch += 1;
            }
            next.pre_release = Vec::new();
        }
        ReleaseType::Premajor => {
            next.major += 1;
            next.minor = 0;
            next.patch = 0;
            next.pre_release = initial_prerelease(preid);
        }
        ReleaseType::Preminor => {
            next.minor += 1;
            next.patch = 0;
            next.pre_release = initial_prerelease(preid);
        }
        ReleaseType::Prepatch => {
            next.patch += 1;
            next.pre_release = initial_prerelease(preid);
        }
        ReleaseType::Prerelease => bump_prerelease(&mut next, preid),
    }
    next
}

/// Whether `version` is already a pre-release of the major it would be
/// bumped to, in which case releasing it only drops the pre-release.
fn releases_pending_major(version: &Version) -> bool {
    !version.pre_release.is_empty() && version.minor == 0 && version.patch == 0
}

/// Whether `version` is already a pre-release of the minor it would be
/// bumped to.
fn releases_pending_minor(version: &Version) -> bool {
    !version.pre_release.is_empty() && version.patch == 0
}

fn bump_prerelease(next: &mut Version, preid: Option<&str>) {
    if next.pre_release.is_empty() {
        next.patch += 1;
        next.pre_release = initial_prerelease(preid);
    } else {
        increment_prerelease(&mut next.pre_release, preid);
    }
}

/// The prerelease identifiers a fresh `pre*` bump starts with: `preid.0`, or
/// a bare `0` without a preid.
fn initial_prerelease(preid: Option<&str>) -> Vec<Identifier> {
    match preid {
        Some(preid) => vec![make_identifier(preid), Identifier::Numeric(0)],
        None => vec![Identifier::Numeric(0)],
    }
}

/// Increment a non-empty prerelease in place: bump the right-most numeric
/// identifier (appending `.0` when there is none), then — when a preid is
/// given — keep the result only if it is already `preid.<number>`, otherwise
/// restart at `preid.0`.
fn increment_prerelease(pre_release: &mut Vec<Identifier>, preid: Option<&str>) {
    let mut bumped = false;
    for identifier in pre_release.iter_mut().rev() {
        if let Identifier::Numeric(number) = identifier {
            *number += 1;
            bumped = true;
            break;
        }
    }
    if !bumped {
        pre_release.push(Identifier::Numeric(0));
    }

    if let Some(preid) = preid {
        let first_matches =
            pre_release.first().is_some_and(|first| identifier_text(first) == preid);
        let second_is_numeric = matches!(pre_release.get(1), Some(Identifier::Numeric(_)));
        if !(first_matches && second_is_numeric) {
            *pre_release = vec![make_identifier(preid), Identifier::Numeric(0)];
        }
    }
}

fn make_identifier(text: &str) -> Identifier {
    match text.parse::<u64>() {
        Ok(number) => Identifier::Numeric(number),
        Err(_) => Identifier::AlphaNumeric(text.to_string()),
    }
}

fn identifier_text(identifier: &Identifier) -> String {
    match identifier {
        Identifier::Numeric(number) => number.to_string(),
        Identifier::AlphaNumeric(text) => text.clone(),
    }
}

fn bump_major(next: &mut Version) {
    if !releases_pending_major(next) {
        next.major += 1;
    }
    next.minor = 0;
    next.patch = 0;
    next.pre_release = Vec::new();
}

pub(super) fn parse_current_version(pkg_dir: &Path, current: &str) -> miette::Result<Version> {
    Version::parse(current).map_err(|_| {
        VersionError::InvalidVersion {
            dir: pkg_dir.display().to_string(),
            version: current.to_owned(),
        }
        .into()
    })
}
