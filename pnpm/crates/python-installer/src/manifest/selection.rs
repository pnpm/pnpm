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
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(super) struct PythonSelection {
    pub(super) extras: Option<Vec<String>>,
    pub(super) groups: Option<Vec<String>>,
    /// Whether the members of the workspace this manifest declares are
    /// resolved as one graph and install into one environment at its
    /// root, rather than each into one of its own.
    #[serde(default)]
    pub(super) shared_environment: bool,
}

impl Manifest {
    /// Whether the workspace this manifest declares asks for one
    /// environment shared by every member.
    pub(in super::super) fn shares_environment(&self) -> bool {
        self.tool.pnpm.python.shared_environment
    }

    pub(in super::super) fn requirements(
        &self,
        config: &Config,
        selection: DependencySelection,
    ) -> Result<Vec<Requirement>> {
        Ok(self.selected_requirements(config)?.into_selected(selection))
    }

    pub(in super::super) fn selected_requirements(
        &self,
        config: &Config,
    ) -> Result<SelectedRequirements> {
        let Some(project) = &self.project else { return Ok(SelectedRequirements::default()) };
        if self.metadata.is_none() {
            project.ensure_static_dependencies()?;
        }
        let extras = self.selected_extras(config)?;
        let mut requirements = self.production_requirements(project, &extras)?;
        let production_count = requirements.len();
        self.expand_configured_groups(config, &mut requirements)?;
        let all = requirements
            .into_iter()
            .map(|requirement| parse_requirement(&requirement))
            .collect::<Result<Vec<_>>>()?;
        Ok(SelectedRequirements { all, production_count })
    }

    fn production_requirements(
        &self,
        project: &super::Project,
        selected: &BTreeSet<ExtraName>,
    ) -> Result<Vec<String>> {
        if self.metadata.is_some() {
            return self.metadata_requirements(selected);
        }
        let mut requirements = project.dependencies.clone();
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

#[derive(Default)]
pub(in super::super) struct SelectedRequirements {
    pub(in super::super) all: Vec<Requirement>,
    production_count: usize,
}

impl SelectedRequirements {
    pub(in super::super) fn selected(&self, selection: DependencySelection) -> &[Requirement] {
        match (selection.production, selection.development) {
            (true, true) => &self.all,
            (true, false) => &self.all[..self.production_count],
            (false, true) => &self.all[self.production_count..],
            (false, false) => &[],
        }
    }

    fn into_selected(mut self, selection: DependencySelection) -> Vec<Requirement> {
        if !selection.development {
            self.all.truncate(self.production_count);
        }
        if !selection.production {
            let count = self.production_count.min(self.all.len());
            self.all.drain(..count);
        }
        self.all
    }
}

#[cfg(test)]
mod tests;
