//! Guard against a store row whose content is not the package the row
//! claims to hold.
//!
//! A `package_index` row is keyed by `<integrity>\t<pkg_id>` — the
//! tarball's checksum plus the `name@version` the resolution was
//! recorded under. The tarball's own `package.json` is the second
//! statement of that identity, and the two can disagree: a hand-edited
//! or merge-mangled lockfile can pair an integrity with the wrong
//! package, and a registry (or a proxy in front of one) can serve a
//! tarball whose manifest does not match the metadata it was listed
//! under. Reusing such a row installs a package under another
//! package's name.
//!
//! [`pkg_content_mismatch`] is the check; `strictStorePkgContentCheck`
//! decides whether a mismatch fails the install or only warns.

/// A store row whose bundled manifest names a different package than
/// its key does. Both fields are rendered `name@version`, with
/// `undefined` standing in for a field the manifest does not carry —
/// the same wording pnpm reports.
#[derive(Debug, PartialEq, Eq)]
pub struct PkgContentMismatch {
    pub expected: String,
    pub actual: String,
}

impl PkgContentMismatch {
    /// The explanation pnpm attaches to both the error and the warning,
    /// so a user who searches either wording finds the same guidance.
    #[must_use]
    pub fn hint(&self) -> String {
        format!(
            "This means that either the lockfile is broken or the package metadata (name and \
             version) inside the package's package.json file doesn't match the metadata in the \
             registry. Expected package: {}. Actual package in the store: {}.",
            self.expected, self.actual,
        )
    }
}

/// Compare the identity `index_key` records against the one the row's
/// bundled `manifest` states, returning the disagreement when there is
/// one.
///
/// `None` — the verdict "nothing to object to" — also covers every case
/// where one of the two identities is unavailable: a key that names no
/// `name@version` package (a URL or git resolution id, or the
/// `<pkg_id>\t{built,not-built}` key git-hosted rows use), a row with no
/// bundled manifest, and a manifest missing the field being compared.
/// Only a field both sides state, and state differently, is a mismatch.
#[must_use]
pub fn pkg_content_mismatch(
    manifest: Option<&serde_json::Value>,
    index_key: &str,
) -> Option<PkgContentMismatch> {
    let (expected_name, expected_version) = key_identity(index_key)?;
    let manifest = manifest?;
    let actual_name = manifest.get("name").and_then(serde_json::Value::as_str);
    let actual_version = manifest.get("version").and_then(serde_json::Value::as_str);

    let name_differs = actual_name.is_some_and(|actual| !same_name(actual, expected_name));
    let version_differs =
        actual_version.is_some_and(|actual| !same_version(actual, expected_version));
    if !name_differs && !version_differs {
        return None;
    }
    Some(PkgContentMismatch {
        expected: format!("{expected_name}@{expected_version}"),
        actual: format!(
            "{}@{}",
            actual_name.unwrap_or("undefined"),
            actual_version.unwrap_or("undefined"),
        ),
    })
}

/// Split a `package_index` key's `pkg_id` half into `name` and
/// `version`.
///
/// Only registry packages have such an id; the version is required to
/// parse as a semver version so that a `pkg_id` which merely contains an
/// `@` — a tarball URL with credentials, say — is not mistaken for one.
fn key_identity(index_key: &str) -> Option<(&str, &str)> {
    let pkg_id = index_key.rsplit_once('\t').map_or(index_key, |(_, pkg_id)| pkg_id);
    let (name, version) = pkg_id.rsplit_once('@')?;
    if name.is_empty() || node_semver::Version::parse(version).is_err() {
        return None;
    }
    Some((name, version))
}

/// Package names are compared case-insensitively, as pnpm compares
/// them. The ASCII pass answers every published name — the registry has
/// rejected non-lowercase names for years, and the ones predating that
/// are ASCII — leaving the allocating fold for names no registry serves.
fn same_name(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right) || left.to_lowercase() == right.to_lowercase()
}

/// Versions are equal when they are the same string, or when they are
/// the same version written differently (`1.0.0` and `v1.0.0`).
fn same_version(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    match (node_semver::Version::parse(left), node_semver::Version::parse(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
