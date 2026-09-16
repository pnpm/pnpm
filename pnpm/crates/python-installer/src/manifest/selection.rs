use super::{DependencySelection, Manifest};
use miette::{IntoDiagnostic, Result, bail};
use pep508_rs::{ExtraName, Requirement};
use pnpm_config::Config;
use pnpm_python_resolver::parse_requirement;
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Default, Clone, Deserialize)]
pub(super) struct Pnpm {
    #[serde(default)]
    pub(super) python: PythonSelection,
}

#[derive(Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PythonSelection {
    pub(super) extras: Option<Vec<String>>,
    pub(super) groups: Option<Vec<String>>,
}

impl Manifest {
    pub(in super::super) fn requirements(
        &self,
        config: &Config,
        selection: DependencySelection,
    ) -> Result<Vec<Requirement>> {
        let Some(project) = &self.project else { return Ok(Vec::new()) };
        if self.metadata.is_none() {
            project.ensure_static_dependencies()?;
        }
        let mut requirements = if selection.production {
            self.production_requirements(project, config)?
        } else {
            Vec::new()
        };
        if selection.development {
            self.expand_configured_groups(config, &mut requirements)?;
        }
        requirements
            .into_iter()
            .map(|requirement| parse_requirement(&requirement))
            .collect()
    }

    fn production_requirements(
        &self,
        project: &super::Project,
        config: &Config,
    ) -> Result<Vec<String>> {
        if self.metadata.is_some() {
            return self.metadata_requirements(config);
        }
        let mut requirements = project.dependencies.clone();
        let selected = self.selected_extras(config)?;
        for (extra, dependencies) in &project.optional_dependencies {
            let extra: ExtraName = extra.parse().into_diagnostic()?;
            if selected.contains(&extra) {
                requirements.extend(dependencies.iter().cloned());
            }
        }
        Ok(requirements)
    }

    pub(super) fn selected_extras(&self, config: &Config) -> Result<BTreeSet<ExtraName>> {
        let explicit = self.tool.pnpm.python.extras.as_ref();
        let selected = crate::build::extra_set(explicit.unwrap_or(&config.python.extras))?;
        let provided = crate::build::extra_set(self.extras())?;
        if explicit.is_some()
            && let Some(extra) = selected.difference(&provided).next()
        {
            bail!("unknown Python project extra: {extra}");
        }
        Ok(selected
            .intersection(&provided)
            .cloned()
            .collect())
    }

    fn expand_configured_groups(
        &self,
        config: &Config,
        requirements: &mut Vec<String>,
    ) -> Result<()> {
        let explicit = self.tool.pnpm.python.groups.as_ref();
        for group in explicit.unwrap_or(&config.python.groups) {
            if explicit.is_none() && !self.groups.contains_key(group) {
                continue;
            }
            self.expand_group(group, &mut Vec::new(), requirements)?;
        }
        Ok(())
    }

    fn expand_group(
        &self,
        group: &str,
        visiting: &mut Vec<String>,
        requirements: &mut Vec<String>,
    ) -> Result<()> {
        if visiting
            .iter()
            .any(|name| name == group)
        {
            bail!("cyclic Python dependency group: {group}");
        }
        let entries = self.groups
            .get(group)
            .ok_or_else(|| miette::miette!("unknown Python dependency group: {group}"))?;
        visiting.push(group.to_string());
        for entry in entries {
            if let Some(requirement) = entry.as_str() {
                requirements.push(requirement.to_string());
            } else if let Some(table) = entry.as_table()
                && table.len() == 1
                && let Some(include) = table.get("include-group").and_then(toml::Value::as_str)
            {
                self.expand_group(include, visiting, requirements)?;
            } else {
                bail!("invalid entry in Python dependency group {group}");
            }
        }
        visiting.pop();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
