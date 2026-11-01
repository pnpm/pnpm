//! The settings only the machine's own configuration sets.

use super::{
    BTreeMap, Config, Deserialize, EnvVar, GetHomeDir, LoadWorkspaceYamlError, MacosBackupSettings,
    Path, Tool, ToolSettings, WorkspaceSettings, expand_home_prefix, overlay, overlay_some,
    overlay_tools, parse_settings, read_readable_settings, read_workspace_document, resolve,
    substitute_optional_string,
};
use crate::{GlobalShimsSetting, SettingsRead};
use std::{collections::HashSet, sync::OnceLock};

/// The settings a repository must not control, because they choose what
/// runs on the machine or where the machine keeps its own state.
///
/// Only the sources the machine's user controls carry them: the global
/// `config.yaml`, `PNPM_CONFIG_*`, the command line, and `updateConfig`
/// hooks, which already run as the user. A project's `pnpm-workspace.yaml`
/// and a config dependency's entry parse into [`WorkspaceSettings`], which
/// has no field for them, and a project file that names one is warned
/// about (see [`crate::refused_keys`]).
#[derive(Debug, Default, PartialEq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror flat config file keys."
    )
)]
pub struct MachineSettings {
    /// CI detection is process state, so a repository must not be able to
    /// turn it off.
    pub ci: Option<bool>,
    /// Resolved by the caller against the default state directory, not by
    /// [`Self::apply_to`]; see [`crate::resolve_configured_state_dir`].
    pub state_dir: Option<String>,
    /// Which scope `pnpm login` claims for the machine.
    pub scope: Option<String>,
    /// A relative value resolves against the directory pnpm runs in, which is
    /// where pnpm itself resolves it. See
    /// [`Config::global_dir`](crate::settings::Config::global_dir).
    pub global_dir: Option<String>,
    /// Resolved like [`Self::global_dir`]. See
    /// [`Config::global_bin_dir`](crate::settings::Config::global_bin_dir).
    pub global_bin_dir: Option<String>,
    pub macos_backup: Option<MacosBackupSettings>,
    /// One layer of the record, merged key-wise into
    /// [`Config::global_shims`](crate::settings::Config::global_shims) rather
    /// than assigned wholesale. A shim decides which program a global command
    /// runs. The `pnpm-workspace.yaml` in the pnpm home sets it too; see
    /// [`Self::load_pnpm_home_global_shims`].
    pub global_shims: Option<GlobalShimsSetting>,
    /// What pnpm is told about the programs it downloads, keyed by tool name.
    /// A mirror decides where the binary pnpm runs an ecosystem with comes
    /// from, and a release's checksums come from the mirror that serves its
    /// files.
    #[serde(default, deserialize_with = "super::deserialize_tools")]
    pub tools: Option<BTreeMap<Tool, ToolSettings>>,
    /// A repository commits the lockfile, so it decides where a symlinked one
    /// points. Letting it also allow the write would let it overwrite any
    /// file the user can write.
    pub symlinked_lockfile_target_dirs: Option<Vec<String>>,
}

impl MachineSettings {
    /// The camelCase keys of these settings, read off the serde
    /// serialization so the set cannot drift from the struct.
    pub(crate) fn keys() -> &'static HashSet<String> {
        static SET: OnceLock<HashSet<String>> = OnceLock::new();
        SET.get_or_init(|| super::serialized_field_keys(&Self::default()))
    }

    /// Whether `text` may set one of these settings, answered without parsing
    /// it. Errs only towards `true`.
    pub(crate) fn may_be_named_in(text: &str) -> bool {
        Self::keys().iter().any(|key| may_name(text, key))
    }

    /// The `globalShims` of the `pnpm-workspace.yaml` in `pnpm_home`, the one
    /// machine setting that file carries: `pnpm add -g` anchors its config
    /// at the pnpm home, which the user owns.
    pub fn load_pnpm_home_global_shims<Sys: EnvVar>(
        pnpm_home: &Path,
        settings_read: SettingsRead,
    ) -> Result<Option<GlobalShimsSetting>, LoadWorkspaceYamlError> {
        let Some(document) = read_workspace_document(pnpm_home)? else { return Ok(None) };
        if !may_name(&document.text, "globalShims") {
            return Ok(None);
        }
        let read = parse_settings::<Sys, Self>;
        let settings = if settings_read.skips_unreadable() {
            read_readable_settings(&document.text, read, settings_read.required_settings())
        } else {
            read(&document.text)
        }
        .map_err(|source| LoadWorkspaceYamlError::ParseYaml { path: document.path, source })?;
        Ok(settings.global_shims)
    }

    /// Expand `${VAR}` in the string settings.
    pub fn substitute_env<Sys: EnvVar>(&mut self) {
        substitute_optional_string::<Sys>(&mut self.scope);
        substitute_optional_string::<Sys>(&mut self.state_dir);
        substitute_optional_string::<Sys>(&mut self.global_dir);
        substitute_optional_string::<Sys>(&mut self.global_bin_dir);
    }

    /// See [`WorkspaceSettings::expand_home_prefixes`].
    pub(crate) fn expand_home_prefixes<Sys: GetHomeDir>(&mut self) {
        let single_dirs = [&mut self.global_dir, &mut self.global_bin_dir];
        let listed_dirs = self.symlinked_lockfile_target_dirs.iter_mut().flatten();
        for dir in single_dirs
            .into_iter()
            .flatten()
            .chain(listed_dirs)
        {
            if let Some(expanded) = expand_home_prefix::<Sys>(dir) {
                *dir = expanded;
            }
        }
    }

    /// Apply every set field except [`Self::state_dir`] onto `config`, the
    /// way [`WorkspaceSettings::apply_to`] does.
    pub fn apply_to(self, config: &mut Config, base_dir: &Path) {
        let Self {
            ci,
            state_dir: _,
            scope,
            global_dir,
            global_bin_dir,
            macos_backup,
            global_shims,
            tools,
            symlinked_lockfile_target_dirs,
        } = self;
        overlay(&mut config.ci, ci);
        overlay_some(&mut config.scope, scope);
        if let Some(dir) = global_dir {
            config.global_dir = Some(resolve(base_dir, &dir));
        }
        if let Some(dir) = global_bin_dir {
            config.global_bin_dir = Some(resolve(base_dir, &dir));
        }
        if let Some(macos_backup) = macos_backup {
            overlay(&mut config.macos_backup.exclude_modules_dir, macos_backup.exclude_modules_dir);
            overlay(&mut config.macos_backup.exclude_store_dir, macos_backup.exclude_store_dir);
        }
        // Merged key-wise instead of replaced, so a layer can flip one package
        // without restating the defaults.
        if let Some(global_shims) = global_shims {
            config.global_shims.apply(&global_shims);
        }
        overlay_tools(&mut config.tools, tools);
        if let Some(dirs) = symlinked_lockfile_target_dirs {
            config.symlinked_lockfile_target_dirs = absolute_dirs(dirs);
        }
    }
}

/// The settings of a source the machine's user controls, which may carry
/// the [`MachineSettings`] as well as the [`WorkspaceSettings`] every source
/// may carry. The file format stays flat: both halves read the same keys.
#[derive(Debug, Default, PartialEq)]
pub struct TrustedSettings {
    pub settings: WorkspaceSettings,
    pub machine: MachineSettings,
}

impl TrustedSettings {
    /// Expand `${VAR}` in every string setting, request destinations
    /// included. See [`WorkspaceSettings::substitute_env_trusted`].
    pub fn substitute_env_trusted<Sys: EnvVar>(&mut self) {
        self.settings.substitute_env_trusted::<Sys>();
        self.machine.substitute_env::<Sys>();
    }

    /// See [`WorkspaceSettings::expand_home_prefixes`].
    pub(crate) fn expand_home_prefixes<Sys: GetHomeDir>(&mut self) {
        self.settings.expand_home_prefixes::<Sys>();
        self.machine.expand_home_prefixes::<Sys>();
    }

    /// Apply both halves onto `config`. See [`WorkspaceSettings::apply_to`].
    pub fn apply_to(self, config: &mut Config, base_dir: &Path) {
        self.settings.apply_to(config, base_dir);
        self.machine.apply_to(config, base_dir);
    }

    /// Both halves as one flat JSON object, in the shape a config file has.
    pub fn to_json(&self) -> serde_json::Result<serde_json::Value> {
        let mut value = serde_json::to_value(&self.settings)?;
        if let (serde_json::Value::Object(map), serde_json::Value::Object(machine)) =
            (&mut value, serde_json::to_value(&self.machine)?)
        {
            map.extend(machine);
        }
        Ok(value)
    }

    /// Read both halves out of one flat JSON object, ignoring keys neither
    /// has.
    pub fn from_json(value: &serde_json::Value) -> serde_json::Result<Self> {
        Ok(Self {
            settings: WorkspaceSettings::deserialize(value)?,
            machine: MachineSettings::deserialize(value)?,
        })
    }
}

/// Whether YAML `text` may hold `key`. A double-quoted key may spell any
/// character as an escape sequence, so a text with a backslash may hold any
/// key.
fn may_name(text: &str, key: &str) -> bool {
    text.contains(key) || text.contains('\\')
}

/// The absolute entries of `symlinkedLockfileTargetDirs`. A relative entry is
/// dropped: the global config and the environment resolve relative paths
/// against the working directory, which may be a repository's checkout.
fn absolute_dirs(dirs: Vec<String>) -> Vec<std::path::PathBuf> {
    dirs.into_iter()
        .map(std::path::PathBuf::from)
        .filter(|dir| {
            let absolute = dir.is_absolute();
            if !absolute {
                let dir = dir.display();
                tracing::warn!(
                    target: "pacquet::config",
                    "Ignoring {dir} in symlinkedLockfileTargetDirs because it is not an absolute path.",
                );
            }
            absolute
        })
        .collect()
}
