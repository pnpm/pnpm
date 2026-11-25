//! Where build decisions are recorded and read back.

use super::ApprovalDecision;
use crate::cli_args::config::ConfigError;
use miette::{
    Context,
    IntoDiagnostic,
};
use pnpm_config::{
    Config,
    GLOBAL_CONFIG_YAML_FILENAME,
    TrustedSettings,
    WORKSPACE_MANIFEST_FILENAME,
    WorkspaceSettings,
};
use pnpm_workspace_manifest_writer::{
    PermissionValue,
    is_owned_by_config_dependency,
    set_allow_builds,
    set_global_packages_permissions,
    set_permissions,
    update_workspace_setting,
};
use std::{
    collections::BTreeMap,
    path::{
        Path,
        PathBuf,
    },
};

/// Where build decisions are recorded.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ApprovalStore<'a> {
    /// The `pnpm-workspace.yaml` of this directory: the workspace root, or
    /// the project outside a workspace.
    Workspace(&'a Path),
    /// `globalPackagesConfig.permissions` in the global `config.yaml`.
    GlobalPackages(GlobalPackagesStore<'a>),
}

/// The files that hold the build decisions of the global packages.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GlobalPackagesStore<'a> {
    /// The directory of the global `config.yaml`.
    pub(crate) config_dir: &'a Path,
    /// The global packages directory, whose `pnpm-workspace.yaml` is still
    /// read for build decisions, deprecated in favor of
    /// `globalPackagesConfig`.
    pub(crate) global_pkg_dir: &'a Path,
}

impl<'a> GlobalPackagesStore<'a> {
    /// The store of `config`'s global `config.yaml`.
    pub(crate) fn of(config: &'a Config, global_pkg_dir: &'a Path) -> Result<Self, ConfigError> {
        let config_dir = config.config_dir.as_deref().ok_or(ConfigError::NoGlobalConfigDir)?;
        Ok(Self { config_dir, global_pkg_dir })
    }
}

impl ApprovalStore<'_> {
    /// Record the decisions of an `approve-builds` run.
    pub(crate) fn write_decision(self, decision: &ApprovalDecision) -> miette::Result<()> {
        let decisions = decision.decisions
            .iter()
            .map(|(pkg, &allowed)| (pkg.as_str(), allowed));
        match self {
            Self::Workspace(dir) => set_permissions(
                dir,
                decisions.map(|(pkg, allowed)| (pkg, PermissionValue::Build(allowed))),
            )
            .into_diagnostic(),
            Self::GlobalPackages(store) => store.write(decisions),
        }
    }

    /// Write permission decisions from `pnpm approve`/`pnpm permissions approve`.
    pub(crate) fn write_permission_decisions(
        self,
        decisions: &BTreeMap<String, crate::cli_args::permissions::decisions::PackageDecisions>,
    ) -> miette::Result<()> {
        let entries = decisions
            .iter()
            .flat_map(|(pkg, decided)| {
                decided.build
                    .map(|allowed| (pkg.as_str(), PermissionValue::Build(allowed)))
                    .into_iter()
                    .chain(
                        decided.skills
                            .clone()
                            .map(|skills| (pkg.as_str(), PermissionValue::Skills(skills))),
                    )
            });
        match self {
            Self::Workspace(dir) => set_permissions(dir, entries).into_diagnostic(),
            Self::GlobalPackages(store) => store.write_permissions(entries),
        }
    }

    /// Write `skills.dirs` to the store's config.
    pub(crate) fn write_skills_dirs(self, dirs: &[String]) -> miette::Result<()> {
        match self {
            Self::Workspace(dir) => {
                pnpm_workspace_manifest_writer::set_skills_dirs(dir, dirs).into_diagnostic()
            }
            Self::GlobalPackages(store) => store.write_skills_dirs(dirs),
        }
    }

    /// Record the decisions of `--allow-build`.
    pub(crate) fn write_allow_builds<'entry>(
        self,
        entries: impl IntoIterator<Item = (&'entry str, bool)>,
    ) -> miette::Result<()> {
        match self {
            Self::Workspace(dir) => set_allow_builds(dir, entries).into_diagnostic(),
            Self::GlobalPackages(store) => store.write(entries),
        }
    }

    /// `config` with the decisions this store records laid over it.
    pub(crate) fn config_with_approvals(self, config: &Config) -> miette::Result<&'static Config> {
        let mut cfg = config.clone();
        let settings = match self {
            Self::Workspace(dir) => WorkspaceSettings::find_and_load(dir)
                .map_err(miette::Report::new)
                .wrap_err("load approved install builds")?
                .map(|(_, settings)| settings),
            Self::GlobalPackages(store) => store.load_global_packages()?,
        };
        if let Some(settings) = settings {
            cfg.allow_builds.extend(settings.decided_build_approvals().unwrap_or_default());
            cfg.allow_skills.extend(settings.decided_skill_approvals().unwrap_or_default());
            // The approval run may have just written `skills.dirs`.
            if let Some(dirs) = settings.skills.and_then(|skills| skills.dirs) {
                cfg.skills_dirs = Some(dirs);
            }
        }
        Ok(Config::leak(cfg))
    }
}

impl GlobalPackagesStore<'_> {
    /// Write `entries` to `globalPackagesConfig.permissions`, moving the
    /// decisions the global packages directory's `pnpm-workspace.yaml` still
    /// holds along with them. A decision already in `globalPackagesConfig`
    /// outranks the one it would move.
    fn write<'entry>(
        self,
        entries: impl IntoIterator<Item = (&'entry str, bool)>,
    ) -> miette::Result<()> {
        self.write_permissions(
            entries
                .into_iter()
                .map(|(pkg, allowed)| (pkg, PermissionValue::Build(allowed))),
        )
    }

    /// Write capability decisions and migrate editable legacy global settings.
    fn write_permissions<'entry>(
        self,
        entries: impl IntoIterator<Item = (&'entry str, PermissionValue)>,
    ) -> miette::Result<()> {
        let movable = self.movable_legacy_settings()?;
        let recorded = self.load_global_packages()?;
        let mut decisions = legacy_permission_decisions(movable.as_ref());
        apply_recorded_permissions(&mut decisions, recorded.as_ref());
        apply_permission_entries(&mut decisions, entries);
        set_global_packages_permissions(
            &self.config_dir.join(GLOBAL_CONFIG_YAML_FILENAME),
            &decisions,
        )
        .into_diagnostic()?;
        remove_moved_legacy_settings(self.global_pkg_dir, movable.as_ref())
    }

    /// Write `globalPackagesConfig.skills.dirs` to the global config.
    pub(crate) fn write_skills_dirs(self, dirs: &[String]) -> miette::Result<()> {
        pnpm_workspace_manifest_writer::set_global_packages_skills_dirs(
            &self.config_dir.join(GLOBAL_CONFIG_YAML_FILENAME),
            dirs,
        )
        .into_diagnostic()
    }

    /// The legacy settings whose build decisions may move. A config
    /// dependency that owns them keeps them: pnpm never edits an installed
    /// package, and a copy above them would outlive the package's own
    /// changes. They stay the lowest layer of the build policy instead.
    fn movable_legacy_settings(self) -> miette::Result<Option<WorkspaceSettings>> {
        let Some((_, settings)) = load_legacy_global_settings(self.global_pkg_dir)? else {
            return Ok(None);
        };
        for key in ["allowBuilds", "permissions"] {
            if is_owned_by_config_dependency(self.global_pkg_dir, key).into_diagnostic()? {
                return Ok(None);
            }
        }
        Ok(Some(settings))
    }

    fn load_global_packages(self) -> miette::Result<Option<WorkspaceSettings>> {
        load_global_packages_config(self.config_dir)
    }
}

/// `globalPackagesConfig` as the global `config.yaml` in `config_dir` holds
/// it now.
pub(crate) fn load_global_packages_config(
    config_dir: &Path,
) -> miette::Result<Option<WorkspaceSettings>> {
    Ok(TrustedSettings::load_global(config_dir)
        .map_err(miette::Report::new)
        .wrap_err("load the global configuration")?
        .and_then(|settings| settings.machine.global_packages_config)
        .map(|settings| *settings))
}

/// The `pnpm-workspace.yaml` of the global packages directory, still read
/// for build decisions, deprecated in favor of `globalPackagesConfig`.
pub(crate) fn load_legacy_global_settings(
    global_pkg_dir: &Path,
) -> miette::Result<Option<(PathBuf, WorkspaceSettings)>> {
    Ok(WorkspaceSettings::load_at(global_pkg_dir)
        .map_err(miette::Report::new)
        .wrap_err("load the build decisions of the global packages directory")?
        .map(|settings| (global_pkg_dir.join(WORKSPACE_MANIFEST_FILENAME), settings)))
}

/// The decisions the global packages directory's `pnpm-workspace.yaml`
/// still holds, keyed by package. Its `permissions` outranks its
/// `allowBuilds` for the same package.
fn legacy_permission_decisions(
    movable: Option<&WorkspaceSettings>,
) -> BTreeMap<String, pnpm_config::PackagePermissions> {
    let Some(settings) = movable else { return BTreeMap::new() };
    let mut decisions = settings.permissions
        .clone()
        .map(pnpm_workspace::map_settings::Rules::into_rules)
        .map(BTreeMap::from_iter)
        .unwrap_or_default();
    for (pkg, allowed) in settings.decided_build_approvals().unwrap_or_default() {
        decisions.entry(pkg).or_default().build = Some(pnpm_config::AllowBuild::Decided(allowed));
    }
    decisions
}

/// Overlay recorded decisions on the legacy policy.
fn apply_recorded_permissions(
    decisions: &mut BTreeMap<String, pnpm_config::PackagePermissions>,
    recorded: Option<&WorkspaceSettings>,
) {
    let Some(recorded) = recorded.and_then(|settings| settings.permissions.as_ref()) else {
        return;
    };
    for (pkg, recorded) in recorded.rules() {
        let entry = decisions.entry(pkg.clone()).or_default();
        if recorded.build
            .as_ref()
            .and_then(pnpm_config::AllowBuild::decided)
            .is_some()
        {
            entry.build.clone_from(&recorded.build);
        }
        if recorded.skills.is_some() {
            entry.skills.clone_from(&recorded.skills);
        }
    }
}

/// Overlay the decisions being written on `decisions`.
fn apply_permission_entries<'entry>(
    decisions: &mut BTreeMap<String, pnpm_config::PackagePermissions>,
    entries: impl IntoIterator<Item = (&'entry str, PermissionValue)>,
) {
    for (pkg, value) in entries {
        let entry = decisions.entry(pkg.to_owned()).or_default();
        match value {
            PermissionValue::Build(allowed) => {
                entry.build = Some(pnpm_config::AllowBuild::Decided(allowed));
            }
            PermissionValue::Skills(skills) => {
                entry.skills = Some(skills);
            }
        }
    }
}

/// Delete the legacy settings that moved into `globalPackagesConfig`.
fn remove_moved_legacy_settings(
    global_pkg_dir: &std::path::Path,
    movable: Option<&WorkspaceSettings>,
) -> miette::Result<()> {
    let Some(settings) = movable else { return Ok(()) };
    let moved = [
        ("allowBuilds", settings.allow_builds.is_some()),
        ("permissions", settings.permissions.is_some()),
    ];
    for (key, _) in moved.into_iter().filter(|(_, set)| *set) {
        update_workspace_setting(global_pkg_dir, key, &serde_json::Value::Null).into_diagnostic()?;
    }
    Ok(())
}
