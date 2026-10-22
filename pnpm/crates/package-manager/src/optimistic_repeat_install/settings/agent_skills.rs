use super::{Config, SettingsComparison, WorkspaceStateSettings};

/// The agent skills settings, recorded only when set. pnpm v11 has no such
/// settings.
pub(super) fn agent_skills_settings(config: &Config) -> WorkspaceStateSettings {
    WorkspaceStateSettings {
        allow_skills: (!config.allow_skills.is_empty()).then(|| {
            config.allow_skills
                .iter()
                .map(|(spec, allowed)| (spec.clone(), *allowed))
                .collect()
        }),
        skills_dirs: config.skills_dirs.clone(),
        ..Default::default()
    }
}

impl SettingsComparison<'_> {
    pub(super) fn agent_skills_drift(&self) -> Option<&'static str> {
        let (recorded, live) = (self.recorded, self.live);
        [
            ("allowSkills", recorded.allow_skills != live.allow_skills),
            ("skills.dirs", recorded.skills_dirs != live.skills_dirs),
        ]
        .into_iter()
        .find(|(key, differs)| *differs && !self.ignored.contains(key))
        .map(|(key, _)| key)
    }
}
