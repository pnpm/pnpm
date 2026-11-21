use super::{
    AddGroups, CatalogMode, Config, Context, DependencyGroup, GlobalPackageBinSnapshot,
    GlobalPackageInfo, GlobalUpdateMode, HashMap, HashSet, Lockfile, PackageBinSource, Path,
    PathBuf, RangeSpecStyle, Reporter, State, SupportedArchitectures, Version, WorkspaceSettings,
    add_packages, apply_allow_build, apply_global_build_policy, apply_pins,
    apply_runtime_range_bumps, global_approval_store, infer_local_package_alias,
    installed_versions, is_registry_version_spec, moves_past_declared_range,
    needs_runtime_range_bump, prompt_approve_install_builds, tag_checked_selectors,
    update_selectors,
};
use pnpm_config::{GetHomeDir, Host};

/// The pnpm home a global group installs into.
pub(super) struct GlobalInstallTarget<'a> {
    pub(super) base_config: &'static Config,
    pub(super) global_pkg_dir: &'a Path,
    pub(super) global_bin_dir: &'a Path,
}

pub(super) struct DowngradeCheck {
    pub(super) candidate_resolved: bool,
    pub(super) pins: HashMap<String, String>,
}

impl GlobalInstallTarget<'_> {
    pub(super) async fn resolve_candidate_selectors<Reporter: self::Reporter + 'static>(
        &self,
        pkg: &GlobalPackageInfo,
        install_dir: &Path,
        mode: GlobalUpdateMode<'_>,
        range_spec_style: RangeSpecStyle,
        supported_architectures: Option<SupportedArchitectures>,
    ) -> miette::Result<Vec<String>> {
        let target_selectors = self
            .target_selectors::<Reporter>(pkg, mode)
            .await?;
        let downgrade_check = Box::pin(pins_for_downgrades::<Reporter>(
            self.base_config,
            self.global_pkg_dir,
            install_dir,
            pkg,
            (mode, &target_selectors),
            range_spec_style,
            supported_architectures.clone(),
        ))
        .await?;
        let mut selectors = apply_pins(&pkg.dependencies, target_selectors, &downgrade_check.pins);
        if !downgrade_check.candidate_resolved || !downgrade_check.pins.is_empty() {
            self.run_lockfile_only_install::<Reporter>(
                install_dir,
                &selectors,
                range_spec_style,
                supported_architectures.clone(),
            )
            .await?;
        }
        match mode {
            GlobalUpdateMode::DeclaredRange | GlobalUpdateMode::Tag(_) => {}
            GlobalUpdateMode::Latest => {
                selectors = self
                    .finalize_latest_runtime_selectors::<Reporter>(
                        pkg,
                        install_dir,
                        selectors,
                        &downgrade_check.pins,
                        range_spec_style,
                        supported_architectures,
                    )
                    .await?;
            }
        }
        Ok(selectors)
    }

    /// The selectors `mode` resolves the group's dependencies with, before
    /// any downgrade pin. Under `--tag` a dependency that does not publish
    /// the tag keeps its recorded spec, so it cannot abort the whole group's
    /// update.
    async fn target_selectors<Reporter: self::Reporter>(
        &self,
        pkg: &GlobalPackageInfo,
        mode: GlobalUpdateMode<'_>,
    ) -> miette::Result<Vec<String>> {
        match mode {
            GlobalUpdateMode::Tag(tag)
                if pkg
                    .dependencies
                    .iter()
                    .any(|(_, spec)| is_registry_version_spec(spec)) =>
            {
                tag_checked_selectors::<Reporter>(self.base_config, &pkg.dependencies, tag).await
            }
            _ => Ok(update_selectors(&pkg.dependencies, mode, &HashMap::new())),
        }
    }

    async fn finalize_latest_runtime_selectors<Reporter: self::Reporter + 'static>(
        &self,
        pkg: &GlobalPackageInfo,
        install_dir: &Path,
        selectors: Vec<String>,
        pins: &HashMap<String, String>,
        range_spec_style: RangeSpecStyle,
        supported_architectures: Option<SupportedArchitectures>,
    ) -> miette::Result<Vec<String>> {
        if !needs_runtime_range_bump(&pkg.dependencies, pins) {
            return Ok(selectors);
        }
        let resolved = resolved_direct_versions(install_dir);
        let bumped = apply_runtime_range_bumps(
            &pkg.dependencies,
            &selectors,
            pins,
            &resolved,
            range_spec_style,
        );
        if bumped != selectors {
            self.run_lockfile_only_install::<Reporter>(
                install_dir,
                &bumped,
                range_spec_style,
                supported_architectures,
            )
            .await?;
        }
        Ok(bumped)
    }

    async fn run_lockfile_only_install<Reporter: self::Reporter + 'static>(
        &self,
        install_dir: &Path,
        selectors: &[String],
        range_spec_style: RangeSpecStyle,
        supported_architectures: Option<SupportedArchitectures>,
    ) -> miette::Result<()> {
        Box::pin(run_group_install::<Reporter>(GroupInstall {
            base_config: self.base_config,
            global_pkg_dir: self.global_pkg_dir,
            install_dir,
            selectors,
            range_spec_style,
            supported_architectures,
            allow_build: &[],
            lockfile_only: true,
        }))
        .await?;
        Ok(())
    }
}

/// A freshly installed group, ready to take over the global bins of the
/// groups it replaces.
pub(super) struct GroupActivation<'a> {
    pub(super) install_dir: &'a Path,
    pub(super) pkgs: &'a [PackageBinSource],
    pub(super) dependencies: &'a [(String, String)],
    pub(super) bins_to_skip: &'a HashSet<String>,
    pub(super) retained_bin_names: &'a HashSet<String>,
    pub(super) groups_to_replace: &'a [GlobalPackageBinSnapshot],
    pub(super) protected_bins: &'a HashSet<String>,
    pub(super) hash: &'a str,
}

/// The version to hold each dependency of `pkg` at, for the ones an update would
/// otherwise move backwards. `--latest` resolves the `latest` dist-tag, which
/// points at an older release than the one installed whenever that came from
/// another tag, or from a major that has not been promoted to `latest` yet.
/// `--tag` resolves a tag that may likewise point at an older release.
///
/// The versions are resolved into `install_dir` without installing anything, so
/// a release that is about to be rejected never gets the chance to run its
/// lifecycle scripts. The install that follows reuses the lockfile written here
/// and only re-resolves what a pin changes.
///
/// Only plain version dependencies, `npm:` aliases, and `runtime:` dependencies
/// are considered: every other spec form says where the package comes from, so
/// holding one at a bare version would resolve a different package from the
/// default registry.
pub(super) async fn pins_for_downgrades<Reporter: self::Reporter + 'static>(
    base_config: &'static Config,
    global_pkg_dir: &Path,
    install_dir: &Path,
    pkg: &GlobalPackageInfo,
    (mode, target_selectors): (GlobalUpdateMode<'_>, &[String]),
    range_spec_style: RangeSpecStyle,
    supported_architectures: Option<SupportedArchitectures>,
) -> miette::Result<DowngradeCheck> {
    if mode == GlobalUpdateMode::DeclaredRange {
        return Ok(DowngradeCheck { candidate_resolved: false, pins: HashMap::new() });
    }
    let versions_before = installed_versions(&pkg.install_dir);
    // Nothing to compare a resolution against, so nothing to resolve.
    if !pkg
        .dependencies
        .iter()
        .any(|(alias, spec)| {
            moves_past_declared_range(mode, alias, spec) && versions_before.contains_key(alias)
        })
    {
        return Ok(DowngradeCheck { candidate_resolved: false, pins: HashMap::new() });
    }
    Box::pin(run_group_install::<Reporter>(GroupInstall {
        base_config,
        global_pkg_dir,
        install_dir,
        selectors: target_selectors,
        range_spec_style,
        supported_architectures,
        allow_build: &[],
        lockfile_only: true,
    }))
    .await?;
    let resolved = resolved_direct_versions(install_dir);

    let pins = pkg
        .dependencies
        .iter()
        .filter(|(alias, spec)| moves_past_declared_range(mode, alias, spec))
        .filter_map(|(alias, _)| {
            let before = Version::parse(versions_before.get(alias)?).ok()?;
            let now = resolved.get(alias)?;
            (*now < before).then(|| (alias.clone(), before.to_string()))
        })
        .collect();
    Ok(DowngradeCheck { candidate_resolved: true, pins })
}

/// The version each direct dependency resolved to, read from the lockfile the
/// resolve pass wrote. Covers plain semver, `npm:` aliases, and `runtime:`
/// dependencies.
fn resolved_direct_versions(install_dir: &Path) -> HashMap<String, Version> {
    let Ok(Some(lockfile)) = Lockfile::load_from_path(&install_dir.join(Lockfile::FILE_NAME))
    else {
        return HashMap::new();
    };
    let Some(importer) = lockfile
        .importers
        .get(Lockfile::ROOT_IMPORTER_KEY)
    else {
        return HashMap::new();
    };
    importer
        .dependencies
        .iter()
        .flatten()
        .filter_map(|(alias, resolved)| {
            Some((
                alias.to_string(),
                resolved
                    .version
                    .ver_peer()?
                    .version_semver()?
                    .clone(),
            ))
        })
        .collect()
}

/// What to install into a fresh global group directory. See
/// [`run_group_install`].
pub(super) struct GroupInstall<'a> {
    pub(super) base_config: &'a Config,
    pub(super) global_pkg_dir: &'a Path,
    /// The group's own directory, created by the caller.
    pub(super) install_dir: &'a Path,
    pub(super) selectors: &'a [String],
    pub(super) range_spec_style: RangeSpecStyle,
    pub(super) supported_architectures: Option<SupportedArchitectures>,
    pub(super) allow_build: &'a [String],
    /// Resolve and write the lockfile without linking anything or running a
    /// build. Nothing a resolution is only being inspected for gets the chance
    /// to run its lifecycle scripts.
    pub(super) lockfile_only: bool,
}

/// Install `install.selectors` into `install.install_dir`, returning the leaked
/// per-group [`Config`] (anchored there, saving to `dependencies`). Then run the
/// global build-approval flow.
pub(super) async fn run_group_install<Reporter: self::Reporter + 'static>(
    install: GroupInstall<'_>,
) -> miette::Result<&'static Config> {
    let mut cfg = global_group_config(
        install.base_config,
        install.install_dir,
        install.global_pkg_dir,
        install.supported_architectures,
    )?;
    apply_allow_build(
        &mut cfg,
        install.allow_build,
        global_approval_store(install.base_config, install.global_pkg_dir)?,
    )?;

    let config: &'static Config = Config::leak(cfg);

    let selectors = install
        .selectors
        .iter()
        .map(|selector| infer_local_package_alias(selector))
        .collect::<miette::Result<Vec<_>>>()?;
    let state = State::init(install.install_dir.join("package.json"), config, false)
        .wrap_err("initialize the global install state")?;
    add_packages::<Reporter, _>(
        state,
        &selectors,
        install.range_spec_style,
        None,
        install.lockfile_only,
        config.supported_architectures.clone(),
        AddGroups { save_target: Some([DependencyGroup::Prod]), included: None, save_types: false },
    )
    .await?;

    if !install.lockfile_only {
        prompt_approve_install_builds::<Reporter>(
            config,
            install.install_dir,
            global_approval_store(install.base_config, install.global_pkg_dir)?,
        )
        .await?;
    }
    Ok(config)
}

/// The virtual store dir a group's install keeps its hidden hoisted modules
/// under, `install_dir` being the group's install dir or its hash link.
pub(super) fn group_install_state_dir(install_dir: &Path) -> PathBuf {
    install_dir
        .join("node_modules")
        .join(".pnpm")
}

pub(crate) fn global_group_config(
    base_config: &Config,
    install_dir: &Path,
    global_pkg_dir: &Path,
    supported_architectures: Option<SupportedArchitectures>,
) -> miette::Result<Config> {
    let mut cfg = base_config.clone();
    cfg.modules_dir = install_dir.join("node_modules");
    cfg.install_state_dir = group_install_state_dir(install_dir);
    use_global_virtual_store_by_default(&mut cfg);
    // Persist a `pnpm-lock.yaml` in the group's install dir (pnpm sets
    // `lockfileDir = installDir`). `outdated -g` / `update -g` read these
    // pins to determine the currently-installed versions. A `lockfileDir`
    // the environment set cannot redirect it, pnpm deletes the setting
    // under `--global`.
    cfg.lockfile = true;
    cfg.configured_lockfile_dir = None;
    cfg.lockfile_dir = None;
    // Pin the group's workspace root to its own install dir (pnpm's
    // `rootProjectManifestDir: installDir`, `workspaceDir: undefined`). The
    // install dir sits *under* the global packages dir, which may carry a
    // `pnpm-workspace.yaml`. Leaving this unset would let the install
    // pipeline walk up, adopt that file as the workspace, and then fail
    // trying to enumerate its non-existent root project. Anchoring here
    // keeps the group install an isolated single project.
    cfg.workspace_dir = Some(install_dir.to_path_buf());
    cfg.supported_architectures = supported_architectures;
    merge_persisted_minimum_release_age_excludes(&mut cfg, install_dir)?;

    // A global install is isolated from the caller's project, so it must
    // not inherit that project's dependency-graph configuration. pnpm
    // achieves this by running the install with `cwd` = the pnpm home dir;
    // pacquet clones the caller's already-loaded config, so drop those
    // project-scoped resolution settings explicitly. Inheriting `overrides`
    // is what surfaced as `ERR_PNPM_CATALOG_IN_OVERRIDES`, a repo override
    // referencing a `catalog:` the isolated install (with no catalogs) no
    // longer resolves, and inheriting `catalogMode: strict` would likewise
    // reject the install against an empty catalog.
    cfg.overrides = None;
    cfg.catalogs = None;
    cfg.catalog_mode = CatalogMode::default();
    cfg.package_extensions = None;
    cfg.patched_dependencies = None;
    // The GVS resolution env injected by `Config::current` points at the
    // *caller's* node_modules, which would let the group's lifecycle
    // scripts resolve phantom deps from the caller's tree.
    cfg.extra_env.remove("NODE_PATH");
    cfg.extra_env.remove("NODE_OPTIONS");
    cfg.apply_global_virtual_store_node_path::<Host>();

    cfg.agent_skills_target_root = Some(
        Host::home_dir().ok_or_else(|| miette::miette!("unable to determine home directory"))?,
    );
    apply_global_build_policy(&mut cfg, global_pkg_dir)?;
    // Don't fail the install when a dependency's build is ignored; the
    // global approval prompt (run after the install) records the ignored
    // builds and prompts rather than erroring under `strictDepBuilds`.
    cfg.strict_dep_builds = false;

    Ok(cfg)
}

/// Global groups use the global virtual store unless `virtualStoreType` (or
/// `enableGlobalVirtualStore`) is set explicitly.
fn use_global_virtual_store_by_default(cfg: &mut Config) {
    cfg.enable_global_virtual_store = cfg
        .explicit_settings
        .get("enableGlobalVirtualStore")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    if cfg.enable_global_virtual_store {
        let virtual_store_dir_explicit = cfg
            .explicit_settings
            .contains_key("virtualStoreDir");
        let global_virtual_store_dir_explicit = cfg
            .explicit_settings
            .contains_key("globalVirtualStoreDir");
        cfg.apply_global_virtual_store_derivation(
            virtual_store_dir_explicit,
            global_virtual_store_dir_explicit,
        );
    }
}

fn merge_persisted_minimum_release_age_excludes(
    cfg: &mut Config,
    install_dir: &Path,
) -> miette::Result<()> {
    let Some(settings) = WorkspaceSettings::load_at(install_dir)
        .map_err(miette::Report::new)
        .wrap_err("load the group's minimumReleaseAgeExclude")?
    else {
        return Ok(());
    };
    if let Some(exclude) = settings.minimum_release_age_exclude {
        cfg.minimum_release_age_exclude
            .get_or_insert_with(Vec::new)
            .extend(exclude);
    }
    Ok(())
}
