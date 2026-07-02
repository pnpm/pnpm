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

    pub fn tarball_name_for_version(&self, version: &str) -> String {
        format!("{}-{version}.tgz", self.basename)
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
        Ok((self.tarball_name_for_version(version), version.to_string()))
    }
}

// `:` is rejected because on Windows `C:foo` is a drive-relative *prefix*
// component: `PathBuf::join` treats it as a new path rather than a child
// segment, so a `:`-carrying name or filename could escape the storage or
// cache root. No legitimate package name, semver version, or tarball
// basename carries a `:`.
fn is_safe_segment(segment: &str) -> bool {
    !segment.is_empty()
        && !segment.starts_with('.')
        && !segment.contains('/')
        && !segment.contains('\\')
        && !segment.contains(':')
        && !segment.contains('\0')
        && segment != ".."
}

/// Whether `filename` is safe to use as a single on-disk path segment (no
/// traversal, no separators, no absolute-path or Windows drive prefixes). The
/// uplink tarball path uses it to admit a non-canonical basename preserved
/// from an upstream `dist.tarball` (see `rewrite_tarball_urls`) into the
/// cache layout — the packument match is what authorizes the name; this only
/// keeps it on disk safely.
#[must_use]
pub fn is_safe_path_segment(filename: &str) -> bool {
    is_safe_segment(filename)
}

#[cfg(test)]
mod tests;
