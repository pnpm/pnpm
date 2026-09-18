//! What a resolution depended on, which is what decides whether a
//! lockfile on disk still answers the project.

use super::Target;
use pep508_rs::{MarkerEnvironment, Requirement};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Everything a resolution depended on: what the project asked for, and
/// the environments it was answered for. A server's answer is accepted
/// only when it was for exactly these; a lockfile on disk is replayed on
/// whatever target still installs it — see [`super::Lockfile::applies_to`].
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The pylock.toml tool.pnpm table preserves existing resolution fields alongside dependency rules."
    )
)]
pub struct Inputs {
    requirements: Vec<String>,
    /// The interpreter a lockfile resolved for the running interpreter
    /// was answered for. A lockfile resolved for declared environments
    /// carries neither: no one interpreter stands for them.
    #[serde(skip_serializing_if = "Option::is_none")]
    environment: Option<MarkerEnvironment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<String>>,
    /// The platforms the project declares, as configured.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    platforms: Vec<String>,
    /// The Python versions the project declares, as configured.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    python_versions: Vec<String>,
    /// The projects sharing the environment this lockfile answers for,
    /// as paths relative to it. Empty for a project resolved on its own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    members: Vec<String>,
    index: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    extra_indexes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    overrides: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    registry_packages: BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    constraints: Vec<String>,
}

impl Inputs {
    /// Why a lockfile answered for these inputs does not answer for
    /// `wanted`, when it does not.
    pub(super) fn differs_from(&self, wanted: &Self) -> Option<&'static str> {
        if self.requirements != wanted.requirements {
            return Some("the project's Python requirements changed");
        }
        if self.index != wanted.index
            || self.extra_indexes != wanted.extra_indexes
            || self.registry_packages != wanted.registry_packages
        {
            return Some("the Python index changed");
        }
        if self.overrides != wanted.overrides || self.constraints != wanted.constraints {
            return Some("the Python overrides or constraints changed");
        }
        if self.platforms != wanted.platforms || self.python_versions != wanted.python_versions {
            return Some("the environments the project locks for changed");
        }
        if self.members != wanted.members {
            return Some("the projects sharing the Python environment changed");
        }
        None
    }

    pub fn set_resolution_settings(
        &mut self,
        extra_indexes: &[String],
        overrides: &[Requirement],
        constraints: &[Requirement],
    ) {
        self.extra_indexes = extra_indexes.to_vec();
        self.overrides = normalized(overrides);
        self.constraints = normalized(constraints);
    }

    /// Record authoritative package routes; changes invalidate lockfile replay.
    pub fn set_registry_packages(&mut self, packages: BTreeMap<String, Vec<String>>) {
        self.registry_packages = packages;
    }

    pub fn set_requirements(&mut self, requirements: &[Requirement]) {
        self.requirements = normalized(requirements);
    }

    /// Record the projects whose requirements this resolution answered
    /// together, so a lockfile says which environment it is for.
    pub fn set_members(&mut self, members: Vec<String>) {
        self.members = members;
    }

    /// The projects sharing the environment this resolution answers for.
    #[must_use]
    pub fn members(&self) -> &[String] {
        &self.members
    }

    /// The inputs of a resolution answered for one interpreter.
    #[must_use]
    pub fn new(requirements: &[Requirement], target: &Target, index: &str) -> Self {
        Self {
            requirements: normalized(requirements),
            environment: Some(target.environment.clone()),
            tags: Some(target.tags.clone()),
            platforms: Vec::new(),
            python_versions: Vec::new(),
            members: Vec::new(),
            index: index.to_string(),
            extra_indexes: Vec::new(),
            overrides: Vec::new(),
            registry_packages: BTreeMap::new(),
            constraints: Vec::new(),
        }
    }

    /// The inputs of a resolution answered for the environments the
    /// project declares, which no interpreter of its own stands for.
    #[must_use]
    pub fn declared(
        requirements: &[Requirement],
        platforms: &[String],
        python_versions: &[String],
        index: &str,
    ) -> Self {
        Self {
            requirements: normalized(requirements),
            environment: None,
            tags: None,
            platforms: platforms.to_vec(),
            python_versions: python_versions.to_vec(),
            members: Vec::new(),
            index: index.to_string(),
            extra_indexes: Vec::new(),
            overrides: Vec::new(),
            registry_packages: BTreeMap::new(),
            constraints: Vec::new(),
        }
    }
}

fn normalized(requirements: &[Requirement]) -> Vec<String> {
    let mut requirements = requirements
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    requirements.sort();
    requirements.dedup();
    requirements
}

#[cfg(test)]
mod tests;
