//! The transforms the install applies to a dependency's manifest before the
//! pnpmfile's `readPackage` hook sees it.

pub use package_extender::{
    InvalidPackageExtensionSelector,
    PackageExtender,
};
use pnpm_config::Config;
use serde_json::Value;
use std::sync::Arc;

mod compat_package_extensions;
mod missing_peer_types;
mod package_extender;

/// The pre-`readPackage` chain for dependency manifests, in order:
///
/// 1. pnpm's compatibility database, unless `ignoreCompatibilityDb` is set;
/// 2. the configured `packageExtensions`;
/// 3. `addMissingPeerTypes`.
#[derive(Debug, Clone)]
pub struct DependencyManifestExtensions {
    compat: Option<&'static PackageExtender>,
    configured: Option<Arc<PackageExtender>>,
    add_missing_peer_types: bool,
}

impl DependencyManifestExtensions {
    /// # Errors
    ///
    /// A `packageExtensions` selector whose range does not parse.
    pub fn from_config(config: &Config) -> Result<Self, InvalidPackageExtensionSelector> {
        let configured = match config.package_extensions.as_ref() {
            Some(extensions) => Some(PackageExtender::new(extensions)?),
            None => None,
        };
        Ok(Self {
            compat: (!config.ignore_compatibility_db).then(
                compat_package_extensions::compat_package_extender,
            ),
            configured: configured
                .filter(|extender| !extender.is_empty())
                .map(Arc::new),
            add_missing_peer_types: config.add_missing_peer_types,
        })
    }

    #[must_use]
    pub fn configured(&self) -> Option<&Arc<PackageExtender>> {
        self.configured.as_ref()
    }

    /// `true` when the chain leaves every manifest unchanged.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.compat.is_none() && self.configured.is_none() && !self.add_missing_peer_types
    }

    /// Run the chain on `manifest`, returning the original `Arc` when no step
    /// changes it.
    #[must_use]
    pub fn apply(&self, mut manifest: Arc<Value>) -> Arc<Value> {
        if let Some(compat) = self.compat {
            manifest = compat.apply_to_arc(manifest);
        }
        if let Some(configured) = &self.configured {
            manifest = configured.apply_to_arc(manifest);
        }
        if self.add_missing_peer_types {
            manifest = missing_peer_types::add_missing_peer_types(manifest);
        }
        manifest
    }
}
