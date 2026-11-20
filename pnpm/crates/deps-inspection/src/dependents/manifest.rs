use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
};

use pnpm_package_manifest::{
    DependencyGroup,
    PackageManifest,
    parse_manifest_bytes,
};

use super::DependencyKind;
use crate::{
    TreeNodeId,
    pkg_info::ManifestSource,
};

struct CachedManifest {
    manifest: PackageManifest,
    /// Requested fields before runtime normalization.
    projection: serde_json::Map<String, serde_json::Value>,
}

/// Shares parsed manifests across all reverse trees in one query.
pub(super) struct ManifestProjector<'a> {
    fields: &'a [String],
    resolved: &'a HashMap<TreeNodeId, ManifestSource>,
    cache: RefCell<HashMap<TreeNodeId, Option<Rc<CachedManifest>>>>,
}

impl<'a> ManifestProjector<'a> {
    pub(super) fn new(
        fields: &'a [String],
        resolved: &'a HashMap<TreeNodeId, ManifestSource>,
    ) -> Self {
        Self { fields, resolved, cache: RefCell::default() }
    }

    fn read_manifest(&self, node_id: &TreeNodeId) -> Option<Rc<CachedManifest>> {
        if let Some(manifest) = self.cache.borrow().get(node_id) {
            return manifest.as_ref().map(Rc::clone);
        }
        let manifest = self.load_manifest(node_id).map(Rc::new);
        self.cache
            .borrow_mut()
            .insert(node_id.clone(), manifest.as_ref().map(Rc::clone));
        manifest
    }

    fn load_manifest(&self, node_id: &TreeNodeId) -> Option<CachedManifest> {
        let source = self.resolved.get(node_id)?;
        let path = source.path.join("package.json");
        let bytes = std::fs::read(&path).ok()?;
        let value = parse_manifest_bytes(&bytes).ok()?;
        let mut projection = serde_json::Map::new();
        for field in self.fields {
            if let Some(value) = value.get(field) {
                projection.insert(field.clone(), value.clone());
            }
        }
        Some(CachedManifest { manifest: PackageManifest::from_value(path, value), projection })
    }

    pub(super) fn project(
        &self,
        node_id: &TreeNodeId,
    ) -> Option<serde_json::Map<String, serde_json::Value>> {
        if self.fields.is_empty() {
            return None;
        }
        let manifest = self.read_manifest(node_id)?;
        (!manifest.projection.is_empty()).then(|| manifest.projection.clone())
    }

    pub(super) fn dependency_specifier(
        &self,
        node_id: &TreeNodeId,
        alias: &str,
        kind: DependencyKind,
    ) -> Option<String> {
        let groups = match kind {
            DependencyKind::Peer => {
                [DependencyGroup::Peer, DependencyGroup::Optional, DependencyGroup::Prod]
            }
            DependencyKind::Dependency => {
                [DependencyGroup::Optional, DependencyGroup::Prod, DependencyGroup::Peer]
            }
        };
        self.read_manifest(node_id)?.manifest
            .dependencies(groups)
            .find(|(name, _)| *name == alias)
            .map(|(_, specifier)| specifier.to_string())
    }
}
