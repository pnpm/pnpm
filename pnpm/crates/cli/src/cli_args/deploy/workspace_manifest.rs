use super::{
    Config, HashMap, IntoDiagnostic, Lockfile, Path, PathBuf,
    install::configured_virtual_store_dir, relative_path,
};
use serde_json::{Map, Number, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct DeployPeerSettings {
    pub(super) add_missing_peer_types: bool,
    pub(super) auto_install_peers: bool,
    pub(super) dedupe_peer_dependents: bool,
    pub(super) dedupe_peers: bool,
    pub(super) peers_suffix_max_length: u64,
}

impl DeployPeerSettings {
    fn from_config(config: &Config) -> Self {
        Self {
            add_missing_peer_types: config.add_missing_peer_types,
            auto_install_peers: config.auto_install_peers,
            dedupe_peer_dependents: false,
            dedupe_peers: config.dedupe_peers,
            peers_suffix_max_length: config.peers_suffix_max_length,
        }
    }

    fn apply_to_config(&self, config: &mut Config) {
        config.add_missing_peer_types = self.add_missing_peer_types;
        config.auto_install_peers = self.auto_install_peers;
        config.dedupe_peer_dependents = self.dedupe_peer_dependents;
        config.dedupe_peers = self.dedupe_peers;
        config.peers_suffix_max_length = self.peers_suffix_max_length;
    }

    fn insert_into_manifest(&self, manifest: &mut Map<String, Value>) {
        manifest.insert("autoInstallPeers".to_string(), Value::Bool(self.auto_install_peers));
        manifest.insert(
            "dedupePeerDependents".to_string(),
            Value::Bool(self.dedupe_peer_dependents),
        );
        manifest.insert("dedupePeers".to_string(), Value::Bool(self.dedupe_peers));
        manifest.insert(
            "peersSuffixMaxLength".to_string(),
            Value::Number(Number::from(self.peers_suffix_max_length)),
        );
        if self.add_missing_peer_types {
            manifest.insert("addMissingPeerTypes".to_string(), Value::Bool(true));
        }
    }
}

pub(super) struct DeployLockfileSettings {
    pub(super) auto_dedupe: bool,
    pub(super) exclude_links_from_lockfile: bool,
    pub(super) include_resolution_settings: bool,
    pub(super) integrity_target: pnpm_lockfile::IntegrityTarget,
    pub(super) link_workspace_packages: bool,
}

impl DeployLockfileSettings {
    fn from_config(config: &Config) -> Self {
        Self {
            auto_dedupe: false,
            exclude_links_from_lockfile: config.exclude_links_from_lockfile,
            include_resolution_settings: config.lockfile_include_resolution_settings,
            integrity_target: config.lockfile_integrity_target,
            link_workspace_packages: false,
        }
    }

    fn apply_to_config(&self, config: &mut Config) {
        config.auto_dedupe = self.auto_dedupe;
        config.exclude_links_from_lockfile = self.exclude_links_from_lockfile;
        config.link_workspace_packages = if self.link_workspace_packages {
            pnpm_config::LinkWorkspacePackages::DirectOnly
        } else {
            pnpm_config::LinkWorkspacePackages::Off
        };
        config.lockfile_include_resolution_settings = self.include_resolution_settings;
        config.lockfile_integrity_target = self.integrity_target;
    }

    /// The deploy lockfile keeps the hashes and resolution settings the source lockfile pins.
    fn insert_into_manifest(&self, manifest: &mut Map<String, Value>) {
        manifest.insert(
            "excludeLinksFromLockfile".to_string(),
            Value::Bool(self.exclude_links_from_lockfile),
        );
        let mut lockfile_settings = Map::new();
        if self.integrity_target != pnpm_lockfile::IntegrityTarget::Resource {
            lockfile_settings.insert(
                "integrityTarget".to_string(),
                serde_json::json!(self.integrity_target),
            );
        }
        if self.include_resolution_settings {
            manifest.insert("autoDedupe".to_string(), Value::Bool(self.auto_dedupe));
            manifest.insert(
                "linkWorkspacePackages".to_string(),
                Value::Bool(self.link_workspace_packages),
            );
            lockfile_settings.insert("includeResolutionSettings".to_string(), Value::Bool(true));
        }
        if !lockfile_settings.is_empty() {
            manifest.insert("lockfile".to_string(), Value::Object(lockfile_settings));
        }
    }
}

pub(super) struct DeployStoreSettings {
    pub(super) dedupe_injected_deps: bool,
    pub(super) inject_workspace_packages: bool,
    pub(super) packages: Vec<String>,
    pub(super) virtual_store_dir: Option<String>,
    pub(super) virtual_store_type: &'static str,
}

impl DeployStoreSettings {
    fn from_config(config: &Config) -> Self {
        Self {
            dedupe_injected_deps: false,
            inject_workspace_packages: false,
            packages: vec![".".to_string()],
            virtual_store_dir: configured_virtual_store_dir(config).map(str::to_string),
            virtual_store_type: "project",
        }
    }

    fn apply_to_config(&self, config: &mut Config) {
        config.dedupe_injected_deps = self.dedupe_injected_deps;
        config.inject_workspace_packages = self.inject_workspace_packages;
    }

    fn insert_into_manifest(&self, manifest: &mut Map<String, Value>) {
        manifest.insert("dedupeInjectedDeps".to_string(), Value::Bool(self.dedupe_injected_deps));
        manifest.insert(
            "injectWorkspacePackages".to_string(),
            Value::Bool(self.inject_workspace_packages),
        );
        manifest.insert(
            "packages".to_string(),
            Value::Array(
                self.packages
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
        manifest.insert(
            "virtualStoreType".to_string(),
            Value::String(self.virtual_store_type.to_string()),
        );
        if let Some(virtual_store_dir) = &self.virtual_store_dir {
            manifest.insert(
                "virtualStoreDir".to_string(),
                Value::String(virtual_store_dir.clone()),
            );
        }
    }
}

/// The single self-contained workspace configuration synthesized for a shared
/// lockfile deploy. The deployed `pnpm-workspace.yaml`, the deployed
/// `pnpm-lock.yaml` settings, and the in-memory `deploy_install_config` all
/// project from this struct so the deploy install and post-deploy commands in
/// the deployed directory observe the same workspace settings.
pub(super) struct DeployWorkspaceSettings {
    pub(super) allow_builds: HashMap<String, bool>,
    pub(super) ignore_pnpmfile: bool,
    pub(super) ignored_optional_dependencies: Vec<String>,
    pub(super) lockfile: DeployLockfileSettings,
    pub(super) patched_dependencies: Option<indexmap::IndexMap<String, String>>,
    pub(super) peers: DeployPeerSettings,
    pub(super) store: DeployStoreSettings,
}

impl DeployWorkspaceSettings {
    pub(super) fn from_config(config: &Config) -> Self {
        Self {
            allow_builds: config.allow_builds.clone(),
            ignore_pnpmfile: true,
            ignored_optional_dependencies: config.ignored_optional_dependencies
                .clone()
                .unwrap_or_default(),
            lockfile: DeployLockfileSettings::from_config(config),
            patched_dependencies: None,
            peers: DeployPeerSettings::from_config(config),
            store: DeployStoreSettings::from_config(config),
        }
    }

    pub(super) fn apply_to_lockfile(&self, deploy_lockfile: &mut Lockfile) {
        let Some(settings) = deploy_lockfile.settings.as_mut() else {
            return;
        };
        settings.inject_workspace_packages = self.store.inject_workspace_packages;
        let resolution = &mut settings.resolution;
        if resolution.dedupe_injected_deps.is_some() {
            resolution.dedupe_injected_deps = Some(self.store.dedupe_injected_deps);
        }
        if resolution.dedupe_peer_dependents.is_some() {
            resolution.dedupe_peer_dependents = Some(self.peers.dedupe_peer_dependents);
        }
        if resolution.auto_dedupe.is_some() {
            resolution.auto_dedupe = Some(self.lockfile.auto_dedupe);
        }
        if resolution.link_workspace_packages.is_some() {
            resolution.link_workspace_packages =
                Some(Value::Bool(self.lockfile.link_workspace_packages));
        }
    }

    pub(super) fn apply_to_config(&self, config: &mut Config, deploy_dir: &Path) {
        config.workspace_dir = Some(deploy_dir.to_path_buf());
        config.allow_builds.clone_from(&self.allow_builds);
        config.ignore_pnpmfile = self.ignore_pnpmfile;
        config.ignored_optional_dependencies = Some(self.ignored_optional_dependencies.clone());
        config.patched_dependencies.clone_from(&self.patched_dependencies);
        self.peers.apply_to_config(config);
        self.lockfile.apply_to_config(config);
        self.store.apply_to_config(config);
        config.isolate_local_directory_imports = true;
        config.overrides = None;
        config.package_extensions = None;
        config.config_dependencies = None;
        config.catalogs = None;
    }

    pub(super) fn to_workspace_manifest(&self) -> miette::Result<Map<String, Value>> {
        let mut manifest = Map::new();
        manifest.insert("ignorePnpmfile".to_string(), Value::Bool(self.ignore_pnpmfile));
        manifest.insert(
            "ignoredOptionalDependencies".to_string(),
            Value::Array(
                self.ignored_optional_dependencies
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
        self.peers.insert_into_manifest(&mut manifest);
        self.lockfile.insert_into_manifest(&mut manifest);
        self.store.insert_into_manifest(&mut manifest);
        if let Some(patched_dependencies) = &self.patched_dependencies {
            manifest.insert(
                "patchedDependencies".to_string(),
                serde_json::to_value(patched_dependencies).into_diagnostic()?,
            );
        }
        if !self.allow_builds.is_empty() {
            manifest.insert(
                "allowBuilds".to_string(),
                serde_json::to_value(self.allow_builds.iter().collect::<BTreeMap<_, _>>())
                    .into_diagnostic()?,
            );
        }
        Ok(manifest)
    }
}

/// The `pnpm-workspace.yaml` the deploy writes, and the same settings in
/// the shape the deploy install consumes. The manifest records the
/// self-contained deploy layout plus settings that survive from the source.
pub(super) fn deploy_workspace_settings(
    lockfile: &Lockfile,
    config: &Config,
    lockfile_dir: &Path,
    deploy_dir: &Path,
    deploy_lockfile: &mut Lockfile,
) -> miette::Result<(Map<String, Value>, DeployWorkspaceSettings)> {
    let mut settings = DeployWorkspaceSettings::from_config(config);
    prune_and_relativize_patched_dependencies(
        lockfile,
        config,
        (lockfile_dir, deploy_dir),
        deploy_lockfile,
        &mut settings,
    );
    settings.apply_to_lockfile(deploy_lockfile);
    let workspace_manifest = settings.to_workspace_manifest()?;
    Ok((workspace_manifest, settings))
}

fn prune_and_relativize_patched_dependencies(
    lockfile: &Lockfile,
    config: &Config,
    (lockfile_dir, deploy_dir): (&Path, &Path),
    deploy_lockfile: &mut Lockfile,
    settings: &mut DeployWorkspaceSettings,
) {
    let (Some(lockfile_patches), Some(config_patches)) =
        (lockfile.patched_dependencies.as_ref(), config.patched_dependencies.as_ref())
    else {
        return;
    };
    let groups = pnpm_lockfile::patch_groups_from_hashes(lockfile_patches);
    let applied = groups
        .as_ref()
        .and_then(|groups| pnpm_lockfile::applied_patch_keys(deploy_lockfile, groups));
    let pruned_lockfile = lockfile_patches
        .iter()
        .filter(|(key, _)| is_applied_patch_key(applied.as_ref(), key))
        .map(|(key, hash)| (key.clone(), hash.clone()))
        .collect::<BTreeMap<_, _>>();
    let rewritten =
        relativize_config_patches(config_patches, lockfile_dir, deploy_dir, applied.as_ref());
    deploy_lockfile.patched_dependencies = (!pruned_lockfile.is_empty()).then_some(pruned_lockfile);
    settings.patched_dependencies = (!rewritten.is_empty()).then_some(rewritten);
}

fn is_applied_patch_key(applied: Option<&BTreeSet<&str>>, key: &str) -> bool {
    applied.is_none_or(|keys| keys.contains(key))
}

fn relativize_config_patches(
    config_patches: &indexmap::IndexMap<String, String>,
    lockfile_dir: &Path,
    deploy_dir: &Path,
    applied: Option<&BTreeSet<&str>>,
) -> indexmap::IndexMap<String, String> {
    config_patches
        .iter()
        .filter(|(key, _)| is_applied_patch_key(applied, key))
        .map(|(name, value)| {
            let absolute = if Path::new(value).is_absolute() {
                PathBuf::from(value)
            } else {
                lockfile_dir.join(value)
            };
            (name.clone(), relative_path(deploy_dir, &absolute))
        })
        .collect()
}
