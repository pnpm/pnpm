use super::{
    AllowBuild, Config, Deserialize, HashMap, IndexMap, NAMED_UNRECOGNIZED_SETTINGS,
    UnrecognizedSettings, WorkspaceSettings, decided_allow_builds, overlay_some,
};

/// A capability a [`PackagePermissions`] entry decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PermissionCapability {
    Build,
    Skills,
}

impl PermissionCapability {
    /// The key that names the capability in `pnpm-workspace.yaml`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            PermissionCapability::Build => "build",
            PermissionCapability::Skills => "skills",
        }
    }
}

/// A package's entry in `permissions`: what the package may do, keyed by
/// capability. `false` records a denial, so the package is not offered for
/// approval again.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PackagePermissions {
    /// May run lifecycle scripts. Outranks the package's `allowBuilds`
    /// entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<AllowBuild>,

    /// May have the agent skills it ships linked into the project's agent
    /// skill directories.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills: Option<AllowBuild>,

    /// Capabilities this version of pnpm does not read, kept only until
    /// [`WorkspaceSettings::collect_key_issues`] has named them in its
    /// report and cleared them.
    #[serde(flatten, skip_serializing_if = "IndexMap::is_empty")]
    pub unknown: IndexMap<String, serde_json::Value>,
}

/// `skills` entry: where pnpm links the agent skills of approved packages.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SkillsSettings {
    /// Agent skill directories, relative to the workspace root. When set,
    /// replaces the detected directories. An empty list turns the feature
    /// off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dirs: Option<Vec<String>>,
}

/// Reduce `permissions` to the decided entries of one capability.
#[must_use]
pub fn decided_permissions<Capability>(
    permissions: &IndexMap<String, PackagePermissions>,
    capability: Capability,
) -> HashMap<String, bool>
where
    Capability: Fn(&PackagePermissions) -> Option<&AllowBuild>,
{
    permissions
        .iter()
        .filter_map(|(pkg, entry)| Some((pkg.clone(), capability(entry)?.decided()?)))
        .collect()
}

impl Config {
    /// Turn agent skills off, for an install into a directory no project
    /// agent reads, such as a global package group, the dlx cache, or a
    /// deploy target.
    pub fn disable_agent_skills(&mut self) {
        self.allow_skills.clear();
        self.agent_skills_disabled = true;
    }
}

impl WorkspaceSettings {
    /// The build decisions of this layer: `allowBuilds`, with
    /// `permissions.<pkg>.build` laid over it. `None` when the layer sets
    /// neither.
    #[must_use]
    pub fn decided_build_approvals(&self) -> Option<HashMap<String, bool>> {
        if self.allow_builds.is_none() && self.permissions.is_none() {
            return None;
        }
        let mut approvals = self.allow_builds
            .clone()
            .map(decided_allow_builds)
            .unwrap_or_default();
        if let Some(permissions) = &self.permissions {
            approvals.extend(decided_permissions(permissions, |entry| {
                entry.build.as_ref()
            }));
        }
        Some(approvals)
    }

    /// The `permissions.<pkg>.skills` decisions of this layer. `None` when
    /// the layer has no `permissions`.
    #[must_use]
    pub fn decided_skill_approvals(&self) -> Option<HashMap<String, bool>> {
        self.permissions
            .as_ref()
            .map(|permissions| {
                decided_permissions(permissions, |entry| entry.skills.as_ref())
            })
    }

    /// Apply `allowBuilds`, `permissions` and `skills` onto `config`.
    pub(super) fn apply_permissions(&mut self, config: &mut Config) {
        let duplicated = self.duplicated_build_decisions();
        if !duplicated.is_empty() {
            let duplicated = duplicated.join(", ");
            tracing::warn!(
                target: "pacquet::config",
                r#"These packages are listed in both "allowBuilds" and "permissions": {duplicated}. Their "allowBuilds" entries are ignored in favor of "permissions"."#,
            );
        }
        if let Some(allow_builds) = self.allow_builds.take() {
            config.allow_builds = decided_allow_builds(allow_builds);
        }
        if let Some(permissions) = self.permissions.take() {
            config.allow_builds.extend(decided_permissions(&permissions, |entry| {
                entry.build.as_ref()
            }));
            config.allow_skills =
                decided_permissions(&permissions, |entry| entry.skills.as_ref());
        }
        if let Some(skills) = self.skills.take() {
            overlay_some(&mut config.skills_dirs, skills.dirs);
        }
    }

    /// The packages this layer decides in both `allowBuilds` and
    /// `permissions.<pkg>.build`, sorted.
    pub(super) fn duplicated_build_decisions(&self) -> Vec<String> {
        let (Some(allow_builds), Some(permissions)) = (&self.allow_builds, &self.permissions)
        else {
            return Vec::new();
        };
        let mut duplicated: Vec<String> = permissions
            .iter()
            .filter(|(pkg, entry)| entry.build.is_some() && allow_builds.contains_key(*pkg))
            .map(|(pkg, _)| pkg.clone())
            .collect();
        duplicated.sort();
        duplicated
    }

    /// Take the capabilities this version of pnpm does not read out of
    /// `permissions`, as the paths that name them.
    pub(super) fn take_unknown_permissions(&mut self) -> UnrecognizedSettings {
        let mut report = UnrecognizedSettings::default();
        let Some(permissions) = self.permissions.as_mut() else { return report };
        for (pkg, entry) in permissions.iter_mut() {
            report.total += entry.unknown.len();
            let named = entry.unknown
                .drain(..)
                .take(NAMED_UNRECOGNIZED_SETTINGS.saturating_sub(report.named.len()));
            report.named.extend(named.map(|(capability, _)| {
                format!("permissions['{pkg}'].{capability}")
            }));
        }
        report
    }
}

#[cfg(test)]
mod tests;
