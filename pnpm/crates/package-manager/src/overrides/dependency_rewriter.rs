use crate::overrides::VersionsOverrider;
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Config;
use pnpm_config_parse_overrides::ParseOverridesError;
use pnpm_workspace_projects_graph::{DependencyRewriter, GraphProject};
use serde_json::Value;
use std::path::Path;

/// The `pnpm.overrides` hook the workspace projects graph applies, so an
/// override that points a dependency at a workspace sibling becomes an
/// edge the graph orders by. `Ok(None)` when no overrides are configured.
/// `catalogs` dereferences `catalog:` override values, the same set the
/// install resolves against.
///
/// Relative `link:` / `file:` override targets are anchored where the
/// install anchors them: `lockfileDir` when it is configured, and
/// `workspace_dir` otherwise.
pub fn overrides_dependency_rewriter(
    config: &Config,
    catalogs: &Catalogs,
    workspace_dir: &Path,
) -> Result<Option<VersionsOverrider>, ParseOverridesError> {
    let Some(map) = config.overrides
        .as_ref()
        .filter(|map| !map.is_empty())
    else {
        return Ok(None);
    };
    let parsed = pnpm_config_parse_overrides::parse_overrides_iter(map.iter(), catalogs)?;
    let lockfile_dir = config.lockfile_dir.as_deref().unwrap_or(workspace_dir);
    Ok(Some(VersionsOverrider::new(&parsed, lockfile_dir)))
}

impl DependencyRewriter for VersionsOverrider {
    /// Runs the merged dependency list through [`Self::apply_to_value`]
    /// as the `dependencies` of a manifest carrying the project's name
    /// and version, so parent-scoped keys match the project exactly as
    /// they match its manifest during resolution.
    fn rewrite_dependencies(
        &self,
        project: &dyn GraphProject,
        dependencies: &mut Vec<(String, String)>,
    ) {
        if self.is_empty() || dependencies.is_empty() {
            return;
        }
        let mut manifest = serde_json::Map::new();
        if let Some(name) = project.manifest_name() {
            manifest.insert("name".to_string(), Value::String(name.to_string()));
        }
        if let Some(version) = project.manifest_version() {
            manifest.insert("version".to_string(), Value::String(version.to_string()));
        }
        manifest.insert(
            "dependencies".to_string(),
            dependencies
                .iter()
                .map(|(name, spec)| (name.clone(), Value::String(spec.clone())))
                .collect::<serde_json::Map<_, _>>()
                .into(),
        );
        let mut manifest = Value::Object(manifest);
        self.apply_to_value(&mut manifest, Some(project.root_dir()));
        let Some(rewritten) = manifest.get("dependencies").and_then(Value::as_object) else {
            return;
        };
        dependencies.retain_mut(|(name, spec)| {
            let Some(rewritten_spec) = rewritten.get(name.as_str()).and_then(Value::as_str) else {
                return false;
            };
            *spec = rewritten_spec.to_string();
            true
        });
    }
}
