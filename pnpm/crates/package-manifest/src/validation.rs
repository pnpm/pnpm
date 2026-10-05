use super::{DependencyGroup, PackageManifest, PackageManifestError};

impl PackageManifest {
    /// Reject dependency specifiers that an install cannot resolve. A group
    /// that is not an object declares no dependencies and is accepted. Keep
    /// this separate from reading the file so `pnpm pkg set` can repair a
    /// malformed field and `readPackage` hooks can fix a specifier before
    /// resolution.
    pub fn validate_dependency_types(&self) -> Result<(), PackageManifestError> {
        for group in [
            DependencyGroup::Prod,
            DependencyGroup::Dev,
            DependencyGroup::Optional,
            DependencyGroup::Peer,
        ] {
            let field: &str = group.into();
            let Some(dependencies) = self.value.get(field).and_then(|value| value.as_object())
            else {
                continue;
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
