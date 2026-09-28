//! The rendered name and version a [`ResolvedPackage`] carries.

use super::{PeerDep, ResolvedPackage};
use pnpm_resolving_resolver_base::ResolveResult;
use std::{collections::BTreeMap, sync::Arc};

impl ResolvedPackage {
    /// Build the envelope, rendering [`Self::name`] and [`Self::version`]
    /// from `result` once.
    #[must_use]
    pub fn new(
        id: Arc<str>,
        result: Arc<ResolveResult>,
        peer_dependencies: BTreeMap<String, PeerDep>,
        optional: bool,
        is_leaf: bool,
    ) -> Self {
        let (name, version) = pkg_name_version(&result);
        ResolvedPackage {
            id,
            name: name.into(),
            version: version.into(),
            result,
            peer_dependencies,
            optional,
            is_leaf,
        }
    }
}

/// The package name and version used for peer compatibility checks.
/// Uses the fetched manifest version when the resolver omits
/// [`pnpm_resolving_resolver_base::ResolvedPackageInfo::name_ver`].
pub(crate) fn pkg_name_version(result: &ResolveResult) -> (String, String) {
    let version = result.package.name_ver
        .as_ref()
        .map(|name_ver| name_ver.suffix.to_string())
        .or_else(|| {
            result.package.manifest
                .as_ref()?
                .get("version")?
                .as_str()
                .map(str::to_owned)
        })
        .unwrap_or_else(|| result.id.as_str().to_string());
    (pkg_name(result), version)
}

/// The name half of [`fn@pkg_name_version`], for callers that would
/// discard the version. `PkgName` holds scope and bare name separately,
/// so rendering either half allocates.
pub(crate) fn pkg_name(result: &ResolveResult) -> String {
    if let Some(name_ver) = result.package.name_ver.as_ref() {
        return name_ver.name.to_string();
    }
    result.alias
        .clone()
        .unwrap_or_else(|| result.id.as_str().to_string())
}
