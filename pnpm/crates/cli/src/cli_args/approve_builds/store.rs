//! Where build decisions are recorded and read back.

use super::ApprovalDecision;
use crate::cli_args::config::ConfigError;
use miette::{Context, IntoDiagnostic};
use pnpm_config::{
    Config, GLOBAL_CONFIG_YAML_FILENAME, TrustedSettings, WORKSPACE_MANIFEST_FILENAME,
    WorkspaceSettings,
};
use pnpm_workspace_manifest_writer::{
    PermissionValue, is_owned_by_config_dependency, set_allow_builds,
    set_global_packages_build_permissions, set_permissions, update_workspace_setting,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
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
    /// The global packages directory. Its `pnpm-workspace.yaml` held the
    /// decisions before `globalPackagesConfig`, and is still read.
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
    /// outranks the one it would move. Global installs link no agent skills,
    /// so that file's `permissions` holds build decisions only and moves
    /// whole.
    fn write<'entry>(
        self,
        entries: impl IntoIterator<Item = (&'entry str, bool)>,
    ) -> miette::Result<()> {
        let movable = self.movable_legacy_settings()?;
        let recorded = self
            .load_global_packages()?
            .and_then(|settings| settings.decided_build_approvals())
            .unwrap_or_default();
        let mut decisions: BTreeMap<String, bool> = movable
            .as_ref()
            .and_then(WorkspaceSettings::decided_build_approvals)
            .unwrap_or_default()
            .into_iter()
            .filter(|(pkg, _)| !recorded.contains_key(pkg))
            .collect();
        decisions.extend(
            entries
                .into_iter()
                .map(|(pkg, allowed)| (pkg.to_string(), allowed)),
        );
        set_global_packages_build_permissions(
            &self.config_dir.join(GLOBAL_CONFIG_YAML_FILENAME),
            decisions
                .iter()
                .map(|(pkg, &allowed)| (pkg.as_str(), allowed)),
        )
        .into_diagnostic()?;
        let Some(settings) = movable else { return Ok(()) };
        let moved = [
            ("allowBuilds", settings.allow_builds.is_some()),
            ("permissions", settings.permissions.is_some()),
        ];
        for (key, _) in moved.into_iter().filter(|(_, set)| *set) {
            update_workspace_setting(self.global_pkg_dir, key, &serde_json::Value::Null)
                .into_diagnostic()?;
        }
        Ok(())
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

/// The `pnpm-workspace.yaml` of the global packages directory, which held
/// the build decisions of the global packages before `globalPackagesConfig`.
pub(crate) fn load_legacy_global_settings(
    global_pkg_dir: &Path,
) -> miette::Result<Option<(PathBuf, WorkspaceSettings)>> {
    Ok(WorkspaceSettings::load_at(global_pkg_dir)
        .map_err(miette::Report::new)
        .wrap_err("load the build decisions of the global packages directory")?
        .map(|settings| (global_pkg_dir.join(WORKSPACE_MANIFEST_FILENAME), settings)))
}
