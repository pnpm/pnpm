use super::{
    AddError, AddResolveInputs,
    aliasless::{AliaslessDependency, resolve_aliasless_specifier},
    specifier::{ProtocolSelector, split_name_spec},
};
use pnpm_package_manifest::PackageManifest;
use pnpm_reporter::LogEvent;

/// A selector as parsed: its protocol, and the package an aliasless
/// specifier names once resolved.
pub(super) struct AddSelector {
    pub(super) protocol: Option<ProtocolSelector>,
    aliasless: Option<AliaslessDependency>,
}

impl AddSelector {
    pub(super) async fn parse(
        package_selector: &str,
        manifest: &PackageManifest,
        inputs: &AddResolveInputs<'_, '_>,
    ) -> Result<Self, AddError> {
        let parsed =
            pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency(package_selector);
        let protocol = ProtocolSelector::parse(package_selector)?;
        let aliasless = match (parsed.alias.as_deref(), parsed.bare_specifier.as_deref()) {
            (None, Some(specifier)) if protocol.is_none() => {
                resolve_aliasless_specifier(specifier, inputs, manifest).await?
            }
            _ => None,
        };
        Ok(Self { protocol, aliasless })
    }

    pub(super) fn linked_peer_dependencies_warning(
        &self,
        package_name: &str,
        prefix: &str,
    ) -> Option<LogEvent> {
        let linked_manifest = self.aliasless.as_ref()?.linked_manifest.as_deref()?;
        crate::linked_peer_dependencies_warning(package_name, linked_manifest, prefix)
    }

    pub(super) fn package_name<'s>(&'s self, package_selector: &'s str) -> &'s str {
        match (self.aliasless.as_ref(), self.protocol.as_ref()) {
            (Some(dep), _) => dep.package_name.as_str(),
            (None, Some(protocol)) => protocol.package_name(),
            (None, None) => split_name_spec(package_selector).0,
        }
    }

    pub(super) fn explicit_spec<'s>(&'s self, package_selector: &'s str) -> Option<&'s str> {
        match (self.aliasless.as_ref(), self.protocol.as_ref()) {
            (Some(dep), _) => Some(dep.manifest_specifier.as_str()),
            (None, Some(protocol)) => protocol.explicit_spec(package_selector),
            (None, None) => split_name_spec(package_selector).1,
        }
        // pnpm reads `foo@` as `foo`, which resolves the `tag` setting.
        .filter(|spec| !spec.is_empty())
    }
}
