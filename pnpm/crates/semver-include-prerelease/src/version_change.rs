use node_semver::Version;

/// The kind of semver bump from one version to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionChange {
    None,
    Fix,
    Feature,
    Breaking,
    /// The versions differ only in prerelease or build metadata.
    Unknown,
}

#[must_use]
pub fn classify_version_change(current: &Version, target: &Version) -> VersionChange {
    if current == target {
        VersionChange::None
    } else if target.major != current.major {
        VersionChange::Breaking
    } else if target.minor != current.minor {
        VersionChange::Feature
    } else if target.patch != current.patch {
        VersionChange::Fix
    } else {
        VersionChange::Unknown
    }
}

/// `target` split before the field `change` bumped: at the start for a
/// breaking bump, at the minor field for a feature bump, at the patch
/// field for a fix. `None` when nothing is highlighted.
#[must_use]
pub fn split_at_changed_field(target: &str, change: VersionChange) -> Option<(&str, &str)> {
    let minor_start = || target.find('.').map(|dot| dot + 1);
    let split = match change {
        VersionChange::Breaking => 0,
        VersionChange::Feature => minor_start().unwrap_or(0),
        VersionChange::Fix => minor_start()
            .and_then(|minor| {
                target[minor..]
                    .find('.')
                    .map(|dot| minor + dot + 1)
            })
            .unwrap_or(0),
        VersionChange::None | VersionChange::Unknown => return None,
    };
    Some(target.split_at(split))
}

#[cfg(test)]
mod tests;
