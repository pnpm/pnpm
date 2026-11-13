use super::manifest::merge_catalogs;
use pnpm_catalogs_types::Catalogs;
use std::collections::BTreeSet;

/// What [`super::manifest::prepare_manifest`] decided beyond the manifest it rewrote.
pub(super) struct PreparedManifest {
    pub(super) updated_catalogs: Catalogs,
    pub(super) picked_versions: Vec<PickedVersion>,
    pub(super) requested_aliases: BTreeSet<String>,
}
/// The version an explicit registry specifier for `alias` resolved to, and
/// the specifier the manifest saves for it.
pub(super) struct PickedVersion {
    pub(super) alias: String,
    /// The registry name the version belongs to.
    pub(super) name: String,
    pub(super) version: String,
    pub(super) specifier: String,
}
impl PreparedManifest {
    /// An exact saved specifier already names the picked version, so it needs
    /// no seed.
    pub(super) fn collect(
        dependencies: impl IntoIterator<Item = super::specifier::ResolvedAddedDependency>,
    ) -> Self {
        let mut updated_catalogs = Catalogs::new();
        let mut picked_versions = Vec::new();
        let mut requested_aliases = BTreeSet::new();
        for dependency in dependencies {
            requested_aliases.insert(dependency.package_name.clone());
            merge_catalogs(&mut updated_catalogs, &dependency.updated_catalogs);
            if let Some(picked) = dependency.picked
                && dependency.manifest_specifier != picked.version
            {
                picked_versions.push(PickedVersion {
                    alias: dependency.package_name,
                    name: picked.name,
                    version: picked.version,
                    specifier: dependency.manifest_specifier,
                });
            }
        }
        Self { updated_catalogs, picked_versions, requested_aliases }
    }
}
