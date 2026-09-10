//! Reject `peerDependencies` values no resolver can read as a range.
//!
//! The common mistake is writing the whole dependency, `bar@1.2.3`,
//! where only its range belongs. Left alone, `autoInstallPeers` hoists
//! the raw value into the importer's dependencies and the resolver
//! reads it as a directory path, so the typo reaches `node_modules` as
//! a link to nothing instead of stopping the install.

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_package_manifest::PackageManifest;
use pnpm_resolving_resolver_base::is_acceptable_peer_spec;

/// `ERR_PNPM_INVALID_PEER_DEPENDENCY_SPECIFICATION`: a
/// `peerDependencies` value that is neither a semver range, a
/// `workspace:`/`catalog:` spec, nor a specifier carrying a
/// protocol/registry scheme.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "The peerDependencies field named '{dep_name}' of package '{project_id}' has an invalid value: '{value}'"
)]
#[diagnostic(
    code(ERR_PNPM_INVALID_PEER_DEPENDENCY_SPECIFICATION),
    help(
        "The values in peerDependencies should be a valid semver range, a `workspace:`/`catalog:` spec, or a dependency specifier such as a named-registry (`<registry>:<version>`), `npm:`, `file:`, or git/URL spec"
    )
)]
pub struct InvalidPeerDependencySpecificationError {
    /// The project as the message names it: the manifest's `name`, or
    /// `fallback_id` when it declares none.
    #[error(not(source))]
    pub project_id: String,
    pub dep_name: String,
    pub value: String,
}

/// Check every `peerDependencies` entry `manifest` declares, reporting
/// the first offender. `fallback_id` names the project in the message
/// when the manifest has no `name`.
pub fn validate_peer_dependencies(
    manifest: &PackageManifest,
    fallback_id: &str,
) -> Result<(), InvalidPeerDependencySpecificationError> {
    let Some(peer_dependencies) =
        manifest.value().get("peerDependencies").and_then(serde_json::Value::as_object)
    else {
        return Ok(());
    };
    for (dep_name, value) in peer_dependencies {
        // A non-string value is a malformed manifest rather than a bad
        // range, and the manifest parser answers for it.
        let Some(value) = value.as_str() else { continue };
        if is_acceptable_peer_spec(value) {
            continue;
        }
        let project_id =
            manifest.value().get("name").and_then(serde_json::Value::as_str).unwrap_or(fallback_id);
        return Err(InvalidPeerDependencySpecificationError {
            project_id: project_id.to_string(),
            dep_name: dep_name.clone(),
            value: value.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
