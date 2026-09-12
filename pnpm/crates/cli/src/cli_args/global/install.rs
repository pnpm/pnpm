use super::{
    CatalogMode, Config, Context, DependencyGroup, GlobalPackageBinSnapshot, GlobalPackageInfo,
    HashMap, HashSet, ImporterDepVersion, Lockfile, PackageBinSource, Path, RangeSpecStyle,
    Reporter, State, SupportedArchitectures, Version, WorkspaceSettings, add_packages,
    apply_allow_build, decided_allow_builds, infer_local_package_alias, installed_versions,
    prompt_approve_global_builds, update_selectors,
};

/// The pnpm home a global group installs into.
pub(super) struct GlobalInstallTarget<'a> {
    pub(super) base_config: &'static Config,
    pub(super) global_pkg_dir: &'a Path,
    pub(super) global_bin_dir: &'a Path,
}

/// A freshly installed group, ready to take over the global bins of the
/// groups it replaces.
pub(super) struct GroupActivation<'a> {
    pub(super) install_dir: &'a Path,
    pub(super) pkgs: &'a [PackageBinSource],
    pub(super) dependencies: &'a [(String, String)],
    pub(super) bins_to_skip: &'a HashSet<String>,
    pub(super) groups_to_replace: &'a [GlobalPackageBinSnapshot],
    pub(super) protected_bins: &'a HashSet<String>,
    pub(super) hash: &'a str,
}

/// The version to hold each dependency of `pkg` at, for the ones an update would
/// otherwise move backwards. `--latest` resolves the `latest` dist-tag, which
/// points at an older release than the one installed whenever that came from
/// another tag, or from a major that has not been promoted to `latest` yet.
///
/// The versions are resolved into `install_dir` without installing anything, so
/// a release that is about to be rejected never gets the chance to run its
/// lifecycle scripts. The install that follows reuses the lockfile written here
/// and only re-resolves what a pin changes.
///
/// Only plain version dependencies are considered: every other spec form says
/// where the package comes from, so holding one at a bare version would resolve
/// a different package from the default registry.
pub(super) async fn pins_for_downgrades<Reporter: self::Reporter + 'static>(
    base_config: &'static Config,
    global_pkg_dir: &Path,
    install_dir: &Path,
    pkg: &GlobalPackageInfo,
    latest: bool,
    range_spec_style: RangeSpecStyle,
    supported_architectures: Option<SupportedArchitectures>,
) -> miette::Result<HashMap<String, String>> {
    // Only `--latest` can pick a version outside the recorded range, and only a
    // plain version spec is dropped for it. Everything else resolves within a
    // range the installed version already satisfies.
    if !latest {
        return Ok(HashMap::new());
    }
    let versions_before = installed_versions(&pkg.install_dir);
    // Nothing to compare a resolution against, so nothing to resolve.
    if !pkg
        .dependencies
        .iter()
        .any(|(alias, spec)| is_plain_version_spec(spec) && versions_before.contains_key(alias))
    {
        return Ok(HashMap::new());
    }
    run_group_install::<Reporter>(GroupInstall {
        base_config,
        global_pkg_dir,
        install_dir,
        selectors: &update_selectors(&pkg.dependencies, latest, &HashMap::new()),
        range_spec_style,
        supported_architectures,
        allow_build: &[],
        lockfile_only: true,
    })
    .await?;
    let resolved = resolved_direct_versions(install_dir);

    Ok(pkg
        .dependencies
        .iter()
        .filter(|(_, spec)| is_plain_version_spec(spec))
        .filter_map(|(alias, _)| {
            let before = Version::parse(versions_before.get(alias)?).ok()?;
            let now = resolved.get(alias)?;
            (*now < before).then(|| (alias.clone(), before.to_string()))
        })
        .collect())
}

/// The version each direct dependency resolved to, read from the lockfile the
/// resolve pass wrote. Only the plain-semver shape is reported: it is the only
/// one a plain version spec resolves to, and the only one a pin can hold.
fn resolved_direct_versions(install_dir: &Path) -> HashMap<String, Version> {
    let Ok(Some(lockfile)) = Lockfile::load_from_path(&install_dir.join(Lockfile::FILE_NAME))
    else {
        return HashMap::new();
    };
    let Some(importer) = lockfile.importers.get(Lockfile::ROOT_IMPORTER_KEY) else {
        return HashMap::new();
    };
    importer
        .dependencies
        .iter()
        .flatten()
        .filter_map(|(alias, resolved)| match &resolved.version {
            ImporterDepVersion::Regular(version) => {
                Some((alias.to_string(), version.version_semver()?.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Only a plain version range may be dropped in favor of the bare alias.
/// Every other spec form (`link:`, `file:`, a git or tarball URL, an `npm:`
/// alias, a named registry) also says where the package comes from, so the
/// alias alone would be resolved from the default registry: a different
/// package gets installed, or the lookup 404s and aborts the groups that
/// have not been updated yet.
pub(super) fn is_plain_version_spec(spec: &str) -> bool {
    !spec.contains(':')
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
/// global build-approval flow. Shared by add and update.
pub(super) async fn run_group_install<Reporter: self::Reporter + 'static>(
    install: GroupInstall<'_>,
) -> miette::Result<&'static Config> {
    let mut cfg = global_group_config(
        install.base_config,
        install.install_dir,
        install.global_pkg_dir,
        install.supported_architectures,
    )?;
    apply_allow_build(&mut cfg, install.allow_build, install.global_pkg_dir)?;

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
        Some([DependencyGroup::Prod]),
    )
    .await?;

    if !install.lockfile_only {
        prompt_approve_global_builds::<Reporter>(
            config,
            install.install_dir,
            install.global_pkg_dir,
        )
        .await?;
    }
    Ok(config)
}

pub(super) fn global_group_config(
    base_config: &Config,
    install_dir: &Path,
    global_pkg_dir: &Path,
    supported_architectures: Option<SupportedArchitectures>,
) -> miette::Result<Config> {
    let mut cfg = base_config.clone();
    cfg.modules_dir = install_dir.join("node_modules");
    cfg.virtual_store_dir = install_dir.join("node_modules").join(".pnpm");
    // Each global group is self-contained, so the virtual store lives
    // inside its install dir (never the shared global one).
    cfg.enable_global_virtual_store = false;
    // Persist a `pnpm-lock.yaml` in the group's install dir (pnpm sets
    // `lockfileDir = installDir`). `outdated -g` / `update -g` read these
    // pins to determine the currently-installed versions. A `lockfileDir`
    // the environment set cannot redirect it — pnpm deletes the setting
    // under `--global`.
    cfg.lockfile = true;
    cfg.lockfile_dir = None;
    // Pin the group's workspace root to its own install dir (pnpm's
    // `rootProjectManifestDir: installDir`, `workspaceDir: undefined`). The
    // install dir sits *under* the global packages dir, which carries a
    // `pnpm-workspace.yaml` of global settings (`allowBuilds`, `catalog`,
    // ...). Leaving this unset would let the install pipeline walk up, adopt
    // that file as the workspace, and then fail trying to enumerate its
    // non-existent root project. Anchoring here keeps the group install an
    // isolated single project.
    cfg.workspace_dir = Some(install_dir.to_path_buf());
    cfg.supported_architectures = supported_architectures;

    // A global install is isolated from the caller's project, so it must
    // not inherit that project's dependency-graph configuration. pnpm
    // achieves this by running the install with `cwd` = the pnpm home dir;
    // pacquet clones the caller's already-loaded config, so drop those
    // project-scoped resolution settings explicitly. Inheriting `overrides`
    // is what surfaced as `ERR_PNPM_CATALOG_IN_OVERRIDES` — a repo override
    // referencing a `catalog:` the isolated install (with no catalogs) no
    // longer resolves — and inheriting `catalogMode: strict` would likewise
    // reject the install against an empty catalog.
    cfg.overrides = None;
    cfg.catalogs = None;
    cfg.catalog_mode = CatalogMode::default();
    cfg.package_extensions = None;
    cfg.patched_dependencies = None;
    // The GVS resolution env injected by `Config::current` points at the
    // *caller's* node_modules; the group's own virtual store is
    // project-local (GVS forced off above), so inheriting it would let
    // the group's lifecycle scripts resolve phantom deps from the
    // caller's tree.
    cfg.extra_env.remove("NODE_PATH");
    cfg.extra_env.remove("NODE_OPTIONS");

    // Build-script policy for global installs comes from the global packages
    // directory, never the caller's repo — otherwise a repo-controlled
    // `pnpm-workspace.yaml` could decide which lifecycle scripts run during
    // `add -g` / `update -g`. Drop the inherited repo policy and load the
    // global `allowBuilds` (where the approval prompt persists its
    // decisions) instead.
    cfg.dangerously_allow_all_builds = false;
    cfg.allow_builds.clear();
    if let Some((_, settings)) = WorkspaceSettings::find_and_load(global_pkg_dir)
        .map_err(miette::Report::new)
        .wrap_err("load global allowBuilds")?
    {
        if let Some(allow_builds) = settings.allow_builds {
            cfg.allow_builds = decided_allow_builds(allow_builds);
        }
        if let Some(allow_all) = settings.dangerously_allow_all_builds {
            cfg.dangerously_allow_all_builds = allow_all;
        }
    }
    // Don't fail the install when a dependency's build is ignored; the
    // global approval prompt (run after the install) records the ignored
    // builds and prompts rather than erroring under `strictDepBuilds`.
    cfg.strict_dep_builds = false;

    Ok(cfg)
}
