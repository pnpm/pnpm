use crate::overrides::VersionsOverrider;
use pnpm_catalogs_protocol_parser::parse_catalog_protocol;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_config_parse_overrides::ParseOverridesError;
use pnpm_package_manifest::DependencyGroup;
use pnpm_workspace_projects_graph::{DependencyGroups, DependencyRewriter, GraphProject};
use serde_json::Value;
use std::path::Path;

/// The `pnpm.overrides` hook the workspace projects graph applies, so an
/// override that points a dependency at a workspace sibling becomes an
/// edge the graph orders by. `Ok(None)` when no overrides are configured.
/// `catalogs` dereferences `catalog:` override values, the same set the
/// install resolves against.
///
/// Relative `link:` / `file:` override targets are anchored where the
/// install anchors them, [`Config::root_project_manifest_dir`] with
/// `workspace_dir` as the fallback.
pub fn overrides_dependency_rewriter(
    config: &Config,
    catalogs: &Catalogs,
    workspace_dir: &Path,
) -> Result<Option<VersionsOverrider>, ParseOverridesError> {
    build_overrides_rewriter(config, catalogs, workspace_dir, CatalogValues::Dereference)
}

/// [`overrides_dependency_rewriter`], except that an override whose value
/// is a `catalog:` reference writes that reference, so the rewritten
/// dependencies name the catalog entries they resolve through. A
/// convergence override still writes the entry's version.
pub fn overrides_catalog_reference_rewriter(
    config: &Config,
    catalogs: &Catalogs,
    workspace_dir: &Path,
) -> Result<Option<VersionsOverrider>, ParseOverridesError> {
    build_overrides_rewriter(config, catalogs, workspace_dir, CatalogValues::Keep)
}

#[derive(Clone, Copy)]
enum CatalogValues {
    Dereference,
    Keep,
}

fn build_overrides_rewriter(
    config: &Config,
    catalogs: &Catalogs,
    workspace_dir: &Path,
    catalog_values: CatalogValues,
) -> Result<Option<VersionsOverrider>, ParseOverridesError> {
    let Some(map) = config.overrides
        .as_ref()
        .filter(|map| !map.is_empty())
    else {
        return Ok(None);
    };
    let mut parsed = pnpm_config_parse_overrides::parse_overrides_iter(map.iter(), catalogs)?;
    match catalog_values {
        CatalogValues::Dereference => {}
        CatalogValues::Keep => {
            for (entry, raw_value) in parsed.iter_mut().zip(map.values()) {
                if !entry.converge && parse_catalog_protocol(raw_value).is_some() {
                    entry.new_bare_specifier.clone_from(raw_value);
                }
            }
        }
    }
    let root_dir = config.root_project_manifest_dir(workspace_dir);
    Ok(Some(VersionsOverrider::new(&parsed, root_dir).with_catalogs(catalogs)))
}

impl DependencyRewriter for VersionsOverrider {
    /// Runs the groups through [`Self::apply_to_value`] as the dependency
    /// fields of a manifest carrying the project's name and version, so
    /// every override (parent-scoped, range-scoped, or scoped to one
    /// dependency field) applies as it does to the manifest during
    /// resolution.
    fn rewrite_dependencies(&self, project: &dyn GraphProject, groups: &mut DependencyGroups) {
        if self.is_empty() {
            return;
        }
        let mut manifest = serde_json::Map::new();
        if let Some(name) = project.manifest_name() {
            manifest.insert("name".to_string(), Value::String(name.to_string()));
        }
        if let Some(version) = project.manifest_version() {
            manifest.insert("version".to_string(), Value::String(version.to_string()));
        }
        insert_group(&mut manifest, DependencyGroup::Peer, &groups.peer);
        if let Some(dev) = &groups.dev {
            insert_group(&mut manifest, DependencyGroup::Dev, dev);
        }
        insert_group(&mut manifest, DependencyGroup::Optional, &groups.optional);
        insert_group(&mut manifest, DependencyGroup::Prod, &groups.prod);
        let mut manifest = Value::Object(manifest);
        self.apply_to_value(&mut manifest, Some(project.root_dir()));
        groups.peer = read_group(&manifest, DependencyGroup::Peer);
        if groups.dev.is_some() {
            groups.dev = Some(read_group(&manifest, DependencyGroup::Dev));
        }
        groups.optional = read_group(&manifest, DependencyGroup::Optional);
        groups.prod = read_group(&manifest, DependencyGroup::Prod);
    }
}

fn insert_group(
    manifest: &mut serde_json::Map<String, Value>,
    group: DependencyGroup,
    dependencies: &[(String, String)],
) {
    let map: serde_json::Map<_, _> = dependencies
        .iter()
        .map(|(name, spec)| (name.clone(), Value::String(spec.clone())))
        .collect();
    let key: &'static str = group.into();
    manifest.insert(key.to_string(), map.into());
}

fn read_group(manifest: &Value, group: DependencyGroup) -> Vec<(String, String)> {
    let key: &'static str = group.into();
    manifest
        .get(key)
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(name, spec)| Some((name.clone(), spec.as_str()?.to_string())))
        .collect()
}
