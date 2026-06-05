use crate::error::RegistryError;

/// A package name validated to be safe for use as a filesystem path
/// segment (no traversal, no absolute-path prefixes) and well-formed
/// enough to send upstream.
#[derive(Debug, Clone)]
pub struct PackageName {
    raw: String,
    /// The unscoped portion — for `@scope/name` this is `name`, for
    /// `name` this is the whole thing. Used to validate tarball
    /// filenames, which are always `<basename>-<version>.tgz`.
    basename: String,
}

impl PackageName {
    pub fn parse(raw: &str) -> Result<Self, RegistryError> {
        let invalid = || RegistryError::InvalidPackageName { name: raw.to_string() };
        if raw.is_empty() || raw.len() > 214 {
            return Err(invalid());
        }
        let basename = if let Some(rest) = raw.strip_prefix('@') {
            let (scope, name) = rest.split_once('/').ok_or_else(invalid)?;
            if !is_safe_segment(scope) || !is_safe_segment(name) {
                return Err(invalid());
            }
            name.to_string()
        } else {
            if !is_safe_segment(raw) {
                return Err(invalid());
            }
            raw.to_string()
        };
        Ok(Self { raw: raw.to_string(), basename })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Validate that `filename` is a plausible tarball name for this
    /// package. Two forms are accepted:
    ///
    /// * `<basename>-<version>.tgz` — the canonical disk-and-URL form
    ///   verdaccio uses and what client GETs send (`/foo/-/foo-1.0.0.tgz`).
    /// * `<full-name>-<version>.tgz` — the form `libnpmpublish` puts in
    ///   the `_attachments` key of a publish body for scoped packages
    ///   (`@scope/name-1.0.0.tgz`). For unscoped packages this collapses
    ///   to the first form.
    pub fn validate_tarball_name(&self, filename: &str) -> Result<(), RegistryError> {
        self.canonicalize_tarball_name(filename).map(|_| ())
    }

    /// Validate `filename` and return the canonical disk filename
    /// (`<basename>-<version>.tgz`). Used by the publish handler so
    /// libnpmpublish's `@scope/name-1.0.0.tgz` attachment lands on
    /// disk under the same path the GET endpoint serves.
    pub fn canonicalize_tarball_name(&self, filename: &str) -> Result<String, RegistryError> {
        self.parse_tarball_name(filename).map(|(canonical, _)| canonical)
    }

    /// Like [`Self::canonicalize_tarball_name`] but also returns the
    /// version segment extracted from the filename. The publish
    /// handler uses the version to look up `versions[v].dist` and
    /// verify the tarball's integrity against what the packument
    /// declares.
    pub fn parse_tarball_name(&self, filename: &str) -> Result<(String, String), RegistryError> {
        let invalid = || RegistryError::InvalidTarballName {
            package: self.raw.clone(),
            filename: filename.to_string(),
        };
        let stem = filename.strip_suffix(".tgz").ok_or_else(invalid)?;
        // Try the longer prefix first so that for an unscoped package
        // (where `self.raw == self.basename`) we still match.
        let rest = stem
            .strip_prefix(&self.raw)
            .or_else(|| stem.strip_prefix(&self.basename))
            .ok_or_else(invalid)?;
        let version = rest.strip_prefix('-').ok_or_else(invalid)?;
        if !is_safe_segment(version) {
            return Err(invalid());
        }
        Ok((format!("{}-{}.tgz", self.basename, version), version.to_string()))
    }
}

fn is_safe_segment(segment: &str) -> bool {
    !segment.is_empty()
        && !segment.starts_with('.')
        && !segment.contains('/')
        && !segment.contains('\\')
        && !segment.contains('\0')
        && segment != ".."
}

#[cfg(test)]
mod tests;
