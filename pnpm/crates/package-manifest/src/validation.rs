use super::{DependencyGroup, PackageManifest, PackageManifestError};

impl PackageManifest {
    /// Reject dependency declarations that an install cannot resolve. Keep this
    /// separate from reading the file so `pnpm pkg set` can repair a malformed
    /// field and `readPackage` hooks can fix a specifier before resolution.
    pub fn validate_dependency_types(&self) -> Result<(), PackageManifestError> {
        for group in [
            DependencyGroup::Prod,
            DependencyGroup::Dev,
            DependencyGroup::Optional,
            DependencyGroup::Peer,
        ] {
            let field: &str = group.into();
            let Some(value) = self.value.get(field) else { continue };
            let Some(dependencies) = value.as_object() else {
                return Err(PackageManifestError::InvalidAttribute(format!(
                    "{}: {field} must be an object",
                    self.path.display(),
                )));
            };
            for (name, specifier) in dependencies {
                if !specifier.is_string() {
                    return Err(PackageManifestError::InvalidAttribute(format!(
                        "{}: dependency {name:?} in {field} must have a string specifier",
                        self.path.display(),
                    )));
                }
            }
        }
        Ok(())
    }
}
