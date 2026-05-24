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
        let basename = match raw.strip_prefix('@') {
            Some(rest) => {
                let (scope, name) = rest.split_once('/').ok_or_else(invalid)?;
                if !is_safe_segment(scope) || !is_safe_segment(name) {
                    return Err(invalid());
                }
                name.to_string()
            }
            None => {
                if !is_safe_segment(raw) {
                    return Err(invalid());
                }
                raw.to_string()
            }
        };
        Ok(Self { raw: raw.to_string(), basename })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Validate that `filename` is a plausible tarball name for this
    /// package: the unscoped basename followed by `-<version>.tgz` and
    /// nothing more.
    pub fn validate_tarball_name(&self, filename: &str) -> Result<(), RegistryError> {
        let invalid = || RegistryError::InvalidTarballName {
            package: self.raw.clone(),
            filename: filename.to_string(),
        };
        if !is_safe_segment(filename) || !filename.ends_with(".tgz") {
            return Err(invalid());
        }
        let stem = &filename[..filename.len() - ".tgz".len()];
        let rest = stem.strip_prefix(&self.basename).ok_or_else(invalid)?;
        let version = rest.strip_prefix('-').ok_or_else(invalid)?;
        if version.is_empty() {
            return Err(invalid());
        }
        Ok(())
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
mod tests {
    use super::PackageName;

    #[test]
    fn accepts_unscoped() {
        let name = PackageName::parse("lodash").unwrap();
        assert_eq!(name.as_str(), "lodash");
        name.validate_tarball_name("lodash-4.17.21.tgz").unwrap();
    }

    #[test]
    fn accepts_scoped() {
        let name = PackageName::parse("@types/node").unwrap();
        assert_eq!(name.as_str(), "@types/node");
        name.validate_tarball_name("node-20.0.0.tgz").unwrap();
    }

    #[test]
    fn rejects_traversal() {
        assert!(PackageName::parse("..").is_err());
        assert!(PackageName::parse("foo/../bar").is_err());
        assert!(PackageName::parse("@scope/..").is_err());
    }

    #[test]
    fn rejects_dot_prefix() {
        assert!(PackageName::parse(".hidden").is_err());
    }

    #[test]
    fn rejects_tarball_for_other_package() {
        let name = PackageName::parse("foo").unwrap();
        assert!(name.validate_tarball_name("bar-1.0.0.tgz").is_err());
        assert!(name.validate_tarball_name("../foo-1.0.0.tgz").is_err());
        assert!(name.validate_tarball_name("foo-1.0.0").is_err());
    }
}
