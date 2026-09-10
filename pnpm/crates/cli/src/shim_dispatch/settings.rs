use super::{
    Candidate, Channel, Config, Display, GlobalShims, GlobalShimsSetting, Host,
    LoadWorkspaceYamlError, PACKAGE_MANAGER_ENVS_DIR_NAME, PackageManager, Path, PathBuf,
    ShimPolicy, Value, WorkspaceSettings, create_hex_hash, create_hex_hash_bytes,
    default_config_dir, default_pnpm_home_dir, default_state_dir, local_bin_identity,
    parse_node_specifier, resolve_configured_state_dir, trusted_runtime_config,
    wanted_package_manager,
};

/// The `globalShims` setting at dispatch time, so config edits take
/// effect immediately instead of waiting for the next global install to
/// relink the shims. Layers merge key-wise over the built-in defaults in
/// the order global `config.yaml`, the pnpm home's own
/// `pnpm-workspace.yaml`, then the env override — never a project file
/// and never a discovered ancestor of the pnpm home. (The env override
/// is only as trustworthy as the environment itself; tools like direnv
/// can scope it per directory.) The state directory comes from its default,
/// the global config, and the environment; the pnpm-home manifest cannot
/// redirect machine state.
pub(super) struct TrustedShimSettings {
    pub(super) shims: GlobalShims,
    pub(super) state_dir: PathBuf,
}

pub(super) fn trusted_shim_settings() -> TrustedShimSettings {
    match load_trusted_shim_settings() {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!(
                "pnpm: project-aware global shims are disabled because trusted configuration could not be loaded: {error}",
            );
            let mut shims = GlobalShims::default();
            shims.apply(&GlobalShimsSetting::Toggle(false));
            TrustedShimSettings {
                shims,
                state_dir: default_state_dir::<Host>().unwrap_or_default(),
            }
        }
    }
}

pub(crate) fn global_shims_setting() -> GlobalShims {
    trusted_shim_settings().shims
}

#[derive(Debug, Display)]
pub(crate) enum LoadGlobalShimsSettingError {
    Workspace(LoadWorkspaceYamlError),
    #[display("malformed {env_name} value {value:?}: {source}")]
    Environment {
        env_name: &'static str,
        value: String,
        source: serde_json::Error,
    },
}

fn load_trusted_shim_settings() -> Result<TrustedShimSettings, LoadGlobalShimsSettingError> {
    let mut shims = GlobalShims::default();
    let default_state_dir = default_state_dir::<Host>().unwrap_or_default();
    let mut state_dir = default_state_dir.clone();
    if let Some(config_dir) = default_config_dir::<Host>() {
        let mut settings = WorkspaceSettings::load_global(&config_dir)
            .map_err(LoadGlobalShimsSettingError::Workspace)?;
        if let Some(settings) = settings.as_mut() {
            settings.substitute_env_trusted::<Host>();
            apply_state_dir_setting(
                &mut state_dir,
                settings.state_dir.as_deref(),
                &default_state_dir,
            );
            if let Some(layer) = settings.global_shims.as_ref() {
                shims.apply(layer);
            }
        }
    }
    apply_settings_above_global_config(&mut shims)?;
    let mut env_settings = WorkspaceSettings::from_pnpm_config_env::<Host>();
    env_settings.substitute_env_trusted::<Host>();
    apply_state_dir_setting(&mut state_dir, env_settings.state_dir.as_deref(), &default_state_dir);
    Ok(TrustedShimSettings { shims, state_dir })
}

pub(super) fn apply_state_dir_setting(
    state_dir: &mut PathBuf,
    setting: Option<&str>,
    default_state_dir: &Path,
) {
    let Some(setting) = setting.filter(|setting| !setting.is_empty()) else { return };
    *state_dir = resolve_configured_state_dir(default_state_dir, setting);
}

/// Apply the `globalShims` layers that outrank the global `config.yaml`:
/// `$PNPM_HOME/pnpm-workspace.yaml`, then the environment.
///
/// `pnpm shim` uses this to answer whether the shim it is about to write
/// would ever dispatch, so the command and the dispatcher cannot disagree
/// about which settings are in force.
pub(crate) fn apply_settings_above_global_config(
    shims: &mut GlobalShims,
) -> Result<(), LoadGlobalShimsSettingError> {
    if let Some(home) = default_pnpm_home_dir::<Host>() {
        let settings =
            WorkspaceSettings::load_at(&home).map_err(LoadGlobalShimsSettingError::Workspace)?;
        if let Some(layer) = settings.and_then(|settings| settings.global_shims) {
            shims.apply(&layer);
        }
    }
    for env_name in ["PNPM_CONFIG_GLOBAL_SHIMS", "pnpm_config_global_shims"] {
        if let Ok(value) = std::env::var(env_name)
            && !value.is_empty()
        {
            let layer = serde_json::from_str::<GlobalShimsSetting>(&value).map_err(|source| {
                LoadGlobalShimsSettingError::Environment { env_name, value, source }
            })?;
            shims.apply(&layer);
            break;
        }
    }
    Ok(())
}

/// The context switch only ever substitutes a different version of the
/// same package the user installed globally: the local candidate must be
/// provided by the same-named package as the embedded global target.
/// A project shipping a same-named bin from a *different* package (a
/// lookalike `tsc` from `evil-pkg`) fails the match and the global
/// version runs.
pub(super) fn validate_candidate(
    candidate: Candidate,
    package: &str,
    name: &str,
) -> Option<Candidate> {
    match candidate {
        Candidate::LocalBin { project_dir, bin, .. } => {
            let local = local_bin_identity(&bin, name)?;
            (local.provider.name == package).then_some(Candidate::LocalBin {
                project_dir,
                bin,
                identity: local.fingerprint,
            })
        }
        Candidate::RuntimePin { project_dir, version_spec, manifest_hash, .. } => (package == name)
            .then(|| Candidate::RuntimePin {
                project_dir,
                identity: create_hex_hash(&format!(
                    "runtime\0{name}\0{version_spec}\0{manifest_hash}",
                )),
                version_spec,
                manifest_hash,
            }),
        Candidate::PackageManagerPin { project_dir, pm, version_spec, manifest_hash, .. } => {
            (package == pm.name()).then(|| Candidate::PackageManagerPin {
                project_dir,
                identity: create_hex_hash(&format!(
                    "package-manager\0{}\0{version_spec}\0{manifest_hash}",
                    pm.name(),
                )),
                pm,
                version_spec,
                manifest_hash,
            })
        }
    }
}

/// The version a project's `package.json` pins for the package manager
/// `pm`, with the manifest's hash so an approval is bound to the file it
/// was given for. A pin naming a different package manager is not this
/// shim's business, and a pin without a version cannot be provisioned.
pub(super) fn manifest_package_manager_pin(
    dir: &Path,
    pm: PackageManager,
) -> Option<(String, String)> {
    let bytes = std::fs::read(dir.join("package.json")).ok()?;
    let manifest_hash = create_hex_hash_bytes(&bytes);
    let manifest: Value = serde_json::from_slice(&bytes).ok()?;
    let wanted = wanted_package_manager(&manifest)?;
    (wanted.name == pm.name()).then_some(())?;
    Some((wanted.version?, manifest_hash))
}

/// Whether a package-manager pin may run without the trust gate.
///
/// A package manager published to npm is verified against npm's own
/// signature for its exact `name@version` before it executes — the same
/// standard that lets pnpm switch itself to a project's pin without
/// asking. Bun and Yarn 6 ship as release archives pinned by a publisher
/// checksum, which authenticates the bytes but not the publisher, so they
/// stay behind the gate like the checksum-only runtime channels.
pub(super) fn package_manager_runs_promptless(
    policy: ShimPolicy,
    pm: PackageManager,
    version_spec: &str,
) -> bool {
    match policy {
        ShimPolicy::Always => true,
        ShimPolicy::Auto => matches!(pm.channel(version_spec), Channel::Registry { .. }),
        ShimPolicy::Off | ShimPolicy::Prompt => false,
    }
}

/// Stable Node releases are authenticated by the Node.js release-team keys
/// before the resolver admits their archive. Other Node channels and the
/// Deno/Bun resolvers currently rely on checksums alone, so they stay behind
/// the explicit `all` opt-in.
pub(super) fn is_automatic_runtime(name: &str, version_spec: &str) -> bool {
    // On musl hosts the matching assets come from unofficial-builds
    // without signature verification, so a stable pin is not
    // publisher-authenticated there and stays behind the trust gate.
    name == "node"
        && parse_node_specifier(version_spec)
            .is_ok_and(|specifier| specifier.release_channel == "release")
        && pnpm_detect_libc::detect() != Some(pnpm_detect_libc::Implementation::Musl)
}

/// The version a project's `package.json` pins for the runtime `name`,
/// `devEngines.runtime` first, then `engines.runtime` — the same
/// precedence as the pre-command runtime check. The pin counts whatever
/// its `onFail` policy says: the dispatcher only chooses which version to
/// run, it does not modify the project.
pub(super) fn manifest_runtime_pin(dir: &Path, name: &str) -> Option<(String, String)> {
    let bytes = std::fs::read(dir.join("package.json")).ok()?;
    let manifest_hash = create_hex_hash_bytes(&bytes);
    let manifest: Value = serde_json::from_slice(&bytes).ok()?;
    for engines_field in ["devEngines", "engines"] {
        for entry in runtime_entries(&manifest, engines_field) {
            if let Some(version) = runtime_entry_version(entry, name) {
                return Some((version, manifest_hash));
            }
        }
    }
    None
}

/// The runtime entries one `engines`-style field declares. The field takes
/// either one entry or an array of them.
fn runtime_entries<'manifest>(
    manifest: &'manifest Value,
    engines_field: &str,
) -> Vec<&'manifest Value> {
    let Some(runtime) = manifest.get(engines_field).and_then(|field| field.get("runtime")) else {
        return Vec::new();
    };
    match runtime {
        Value::Array(entries) => entries.iter().collect(),
        single => vec![single],
    }
}

/// The version an entry pins, when it names `name` and pins one at all.
fn runtime_entry_version(entry: &Value, name: &str) -> Option<String> {
    (entry.get("name").and_then(Value::as_str) == Some(name)).then_some(())?;
    let version = entry.get("version").and_then(Value::as_str)?.trim();
    (!version.is_empty()).then(|| version.to_string())
}

/// The configuration package-manager provisioning runs under: pnpm's own
/// trusted layers, anchored inside its state directory so no project's
/// `pnpm-workspace.yaml` can redirect the store the executable comes from.
pub(super) fn trusted_package_manager_config(state_dir: &Path) -> miette::Result<Config> {
    if state_dir.as_os_str().is_empty() {
        return Err(miette::miette!("the pnpm state directory could not be resolved"));
    }
    trusted_runtime_config(&state_dir.join(PACKAGE_MANAGER_ENVS_DIR_NAME))
}
