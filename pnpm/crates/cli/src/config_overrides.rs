use pnpm_config::{
    ColorMode, Config, EnvVar, GLOBAL_LAYOUT_VERSION, GetCurrentDir, GetHomeDir, LinkProbe,
    LinkWorkspacePackages, NodeLinker, PackageImportMethod, PmOnFail, RuntimeOnFail,
    SaveWorkspaceProtocol, TrustPolicy, VerifyDepsBeforeRun, default_state_dir,
    resolve_child_concurrency,
};
use pnpm_fs::lexical_normalize;
use pnpm_store_dir::StoreDir;
use std::{
    collections::{BTreeMap, HashSet},
    ffi::{OsStr, OsString},
    path::Path,
};

pub(crate) fn apply_store_dir_override<Sys>(
    config: &mut Config,
    store_dir: &Path,
    dir: &Path,
) -> miette::Result<()>
where
    Sys: EnvVar + GetCurrentDir + GetHomeDir + LinkProbe,
{
    let workspace_dir = config.workspace_dir.as_deref().unwrap_or(dir).to_path_buf();
    if store_dir.as_os_str().is_empty() {
        config.reset_store_dir_to_default::<Sys>(&workspace_dir);
        config
            .explicit_settings
            .insert("storeDir".to_string(), serde_json::Value::String(String::new()));
        return Ok(());
    }
    let resolved = if let Some(relative) = home_relative_store_dir(store_dir) {
        Sys::home_dir()
            .ok_or_else(|| {
                let store_dir_display = store_dir.display();
                miette::miette!(
                    "Cannot resolve store directory {} because the home directory is unknown",
                    store_dir_display,
                )
            })?
            .join(relative)
    } else if store_dir.is_absolute() {
        store_dir.to_path_buf()
    } else {
        workspace_dir.join(store_dir)
    };
    config.store_dir = StoreDir::from(lexical_normalize(&resolved));
    if let Some(store_dir) = store_dir.to_str() {
        config
            .explicit_settings
            .insert("storeDir".to_string(), serde_json::Value::String(store_dir.to_string()));
    }
    let virtual_store_dir_explicit = config.explicit_settings.contains_key("virtualStoreDir");
    let global_virtual_store_dir_explicit =
        config.explicit_settings.contains_key("globalVirtualStoreDir");
    config.apply_global_virtual_store_derivation(
        virtual_store_dir_explicit,
        global_virtual_store_dir_explicit,
    );
    Ok(())
}

pub(crate) fn apply_state_dir_override<Sys>(config: &mut Config, state_dir: &Path, dir: &Path)
where
    Sys: EnvVar + GetHomeDir,
{
    config.state_dir = if state_dir.as_os_str().is_empty() {
        default_state_dir::<Sys>().unwrap_or_default()
    } else if state_dir.is_absolute() {
        lexical_normalize(state_dir)
    } else {
        lexical_normalize(&dir.join(state_dir))
    };
    if let Some(state_dir) = state_dir.to_str() {
        config
            .explicit_settings
            .insert("stateDir".to_string(), serde_json::Value::String(state_dir.to_string()));
    }
}

fn home_relative_store_dir(store_dir: &Path) -> Option<&Path> {
    let store_dir = store_dir.to_str()?;
    store_dir.strip_prefix("~/").or_else(|| store_dir.strip_prefix(r"~\")).map(Path::new)
}

/// CLI overrides parsed from pnpm's `--config.<key>=<value>` dotted-key
/// syntax. Upstream pnpm uses [`npm-conf`](https://github.com/npm/npm-conf)
/// to translate each `--config.<key>=<value>` token into a runtime config
/// assignment that wins over `.npmrc` and `pnpm-workspace.yaml`; pacquet
/// mirrors that by stripping the same tokens out of argv before clap sees
/// them and re-applying them onto [`Config`] after the file-based layers
/// have run.
///
/// Unknown keys are accepted silently: pnpm exposes a long tail of config
/// keys, and erroring on an unrecognized one would break the moment pnpm
/// adds a new key that pacquet hasn't ported yet. Dropping one is only
/// harmless when pnpm parsed the token first and delegated, leaving the
/// pacquet leg to fall back to the yaml value. When the binary runs
/// standalone there is no other leg, so a setting that changes what gets
/// installed has to be ported here.
#[derive(Debug, Default)]
pub struct ConfigOverrides {
    allow_unused_patches: Option<bool>,
    bail: Option<bool>,
    ci: Option<bool>,
    color: Option<ColorMode>,
    embed_readme: Option<bool>,
    ignore_workspace_root_check: Option<bool>,
    lockfile: Option<bool>,
    optional: Option<bool>,
    package_lock: Option<bool>,
    pending: Option<bool>,
    recursive_install: Option<bool>,
    reverse: Option<bool>,
    shamefully_hoist: Option<bool>,
    shell_emulator: Option<bool>,
    side_effects_cache: Option<bool>,
    side_effects_cache_readonly: Option<bool>,
    skip_manifest_obfuscation: Option<bool>,
    sort: Option<bool>,
    use_beta_cli: Option<bool>,
    registry: Option<String>,
    scope: Option<String>,
    registries: BTreeMap<String, String>,
    child_concurrency: Option<i32>,
    dangerously_allow_all_builds: Option<bool>,
    deploy_all_files: Option<bool>,
    engine_strict: Option<bool>,
    force_legacy_deploy: Option<bool>,
    frozen_store: Option<bool>,
    global_dir: Option<String>,
    hoist: Option<bool>,
    hoist_pattern: Option<Vec<String>>,
    ignore_pnpmfile: Option<bool>,
    ignore_scripts: Option<bool>,
    inject_workspace_packages: Option<bool>,
    link_workspace_packages: Option<LinkWorkspacePackages>,
    lockfile_include_tarball_url: Option<bool>,
    /// `maxsockets`, npm's spelling of [`Self::max_sockets`]. Kept apart
    /// so the canonical spelling can win when one command line carries
    /// both.
    maxsockets: Option<usize>,
    max_sockets: Option<usize>,
    minimum_release_age: Option<u64>,
    minimum_release_age_exclude: Option<Vec<String>>,
    minimum_release_age_ignore_missing_time: Option<bool>,
    minimum_release_age_strict: Option<bool>,
    merge_git_branch_lockfiles: Option<bool>,
    node_experimental_package_map: Option<bool>,
    offline: Option<bool>,
    prefer_frozen_lockfile: Option<bool>,
    prefer_offline: Option<bool>,
    /// The raw `modulesDir` / `virtualStoreDir` spellings, kept unresolved
    /// so [`Config::anchor_lockfile_paths`] can re-resolve them against
    /// whichever directory ends up anchoring the install.
    modules_dir: Option<String>,
    virtual_store_dir: Option<String>,
    node_linker: Option<NodeLinker>,
    optimistic_repeat_install: Option<bool>,
    package_import_method: Option<PackageImportMethod>,
    pm_on_fail: Option<PmOnFail>,
    public_hoist_pattern: Option<Vec<String>>,
    runtime_on_fail: Option<RuntimeOnFail>,
    save_workspace_protocol: Option<SaveWorkspaceProtocol>,
    shared_workspace_lockfile: Option<bool>,
    strict_peer_dependencies: Option<bool>,
    trust_lockfile: Option<bool>,
    trust_policy: Option<TrustPolicy>,
    trust_policy_exclude: Option<Vec<String>>,
    trust_policy_ignore_after: Option<u64>,
    unsafe_perm: Option<bool>,
    verify_deps_before_run: Option<VerifyDepsBeforeRun>,
    verify_store_integrity: Option<bool>,
    virtual_store_only: Option<bool>,
    https_proxy: Option<String>,
    http_proxy: Option<String>,
    no_proxy: Option<String>,
}

impl ConfigOverrides {
    /// Pull `--config.<key>=<value>` tokens and [`BARE_SETTING_FLAGS`]
    /// spellings out of `argv` and collect them. Returns the parsed
    /// overrides together with the remaining argv tokens (in their
    /// original order) for clap to parse.
    pub fn extract<Argv>(argv: Argv) -> (Self, Vec<OsString>)
    where
        Argv: IntoIterator<Item = OsString>,
    {
        let argv = argv.into_iter().collect::<Vec<_>>();
        let passthrough_from = crate::parse_boundary::passthrough_from(&argv);
        let claimed_by_command = crate::parse_boundary::subcommand_option_names(&argv);
        let is_forwarded = |index: usize| passthrough_from.is_some_and(|from| index >= from);
        let mut overrides = Self::default();
        let mut remaining = Vec::new();
        let mut argv = argv.into_iter().enumerate().peekable();
        while let Some((index, arg)) = argv.next() {
            if is_forwarded(index) {
                remaining.push(arg);
                continue;
            }
            // The token after a `--<setting> <value>` pair's flag, when the
            // setting claims it — see [`claims_as_value`]. `None` when the
            // flag ends argv, the token is already the child's, or it is
            // not a value the setting takes, all of which leave the
            // valueless flag for clap to report.
            let mut following = |key: &str| {
                let value = argv
                    .peek()
                    .filter(|&&(index, _)| !is_forwarded(index))
                    .and_then(|(_, token)| token.to_str())
                    .filter(|token| claims_as_value(key, token))
                    .map(str::to_owned)?;
                argv.next();
                Some(value)
            };
            match classify(&arg, &claimed_by_command) {
                ConfigToken::WellFormed { key, value }
                    if matches!(key, "state-dir" | "store-dir") =>
                {
                    remaining.push(OsString::from(format!("--{key}={value}")));
                }
                ConfigToken::WellFormed { key, value } => overrides.set(key, value),
                ConfigToken::BooleanFollows(key) => {
                    overrides.set(key, following(key).as_deref().unwrap_or("true"));
                }
                // A flag whose value is missing — because it ends argv, or
                // because the token after it is one the setting does not
                // take — goes back in place for clap to report; see
                // [`classify`].
                ConfigToken::ValueFollows(key) => match following(key) {
                    Some(value) => overrides.set(key, &value),
                    None => remaining.push(arg),
                },
                ConfigToken::Malformed => {}
                ConfigToken::NotOurs => remaining.push(arg),
            }
        }
        (overrides, remaining)
    }

    fn set(&mut self, key: &str, value: &str) {
        match key {
            "allow-unused-patches" => self.allow_unused_patches = parse_bool(value),
            "bail" => self.bail = parse_bool(value),
            "ci" => self.ci = parse_bool(value),
            "dangerously-allow-all-builds" => {
                self.dangerously_allow_all_builds = parse_bool(value);
            }
            "color" => {
                self.color = parse_bool(value)
                    .map(|enabled| if enabled { ColorMode::Always } else { ColorMode::Never })
                    .or_else(|| parse_enum(value));
            }
            "embed-readme" => self.embed_readme = parse_bool(value),
            "engine-strict" => self.engine_strict = parse_bool(value),
            "frozen-store" => self.frozen_store = parse_bool(value),
            "ignore-workspace-root-check" => {
                self.ignore_workspace_root_check = parse_bool(value);
            }
            "hoist" => self.hoist = parse_bool(value),
            "ignore-pnpmfile" => self.ignore_pnpmfile = parse_bool(value),
            "link-workspace-packages" => {
                self.link_workspace_packages = parse_bool_or_enum(value);
            }
            "lockfile" => self.lockfile = parse_bool(value),
            "lockfile-include-tarball-url" => {
                self.lockfile_include_tarball_url = parse_bool(value);
            }
            "merge-git-branch-lockfiles" => {
                self.merge_git_branch_lockfiles = parse_bool(value);
            }
            "node-experimental-package-map" => {
                self.node_experimental_package_map = parse_bool(value);
            }
            "offline" => self.offline = parse_bool(value),
            "optimistic-repeat-install" => self.optimistic_repeat_install = parse_bool(value),
            "optional" => self.optional = parse_bool(value),
            "package-lock" => self.package_lock = parse_bool(value),
            "pending" => self.pending = parse_bool(value),
            "prefer-frozen-lockfile" => self.prefer_frozen_lockfile = parse_bool(value),
            "prefer-offline" => self.prefer_offline = parse_bool(value),
            "recursive-install" => self.recursive_install = parse_bool(value),
            "reverse" => self.reverse = parse_bool(value),
            "save-workspace-protocol" => {
                self.save_workspace_protocol = parse_bool_or_enum(value);
            }
            "shamefully-hoist" => self.shamefully_hoist = parse_bool(value),
            "shell-emulator" => self.shell_emulator = parse_bool(value),
            "side-effects-cache" => self.side_effects_cache = parse_bool(value),
            "side-effects-cache-readonly" => {
                self.side_effects_cache_readonly = parse_bool(value);
            }
            "skip-manifest-obfuscation" => {
                self.skip_manifest_obfuscation = parse_bool(value);
            }
            "sort" => self.sort = parse_bool(value),
            "strict-peer-dependencies" => self.strict_peer_dependencies = parse_bool(value),
            "trust-lockfile" => self.trust_lockfile = parse_bool(value),
            "unsafe-perm" => self.unsafe_perm = parse_bool(value),
            "use-beta-cli" => self.use_beta_cli = parse_bool(value),
            "verify-store-integrity" => self.verify_store_integrity = parse_bool(value),
            "virtual-store-only" => self.virtual_store_only = parse_bool(value),
            _ => {}
        }
        if key == "registry" {
            self.registry = Some(normalize_registry_url(value));
            return;
        }
        if key == "scope" {
            self.scope = Some(value.to_string());
            return;
        }
        if key == "https-proxy" {
            self.https_proxy = Some(value.to_string());
            return;
        }
        if key == "http-proxy" {
            self.http_proxy = Some(value.to_string());
            return;
        }
        if key == "no-proxy" {
            self.no_proxy = Some(value.to_string());
            return;
        }
        if key == "child-concurrency" {
            self.child_concurrency = value.parse().ok();
            return;
        }
        if key == "deploy-all-files" {
            self.deploy_all_files = parse_bool(value);
            return;
        }
        if key == "force-legacy-deploy" {
            self.force_legacy_deploy = parse_bool(value);
            return;
        }
        if key == "global-dir" {
            self.global_dir = Some(value.to_string());
            return;
        }
        if key == "hoist-pattern" {
            self.hoist_pattern.get_or_insert_default().push(value.to_string());
            return;
        }
        if key == "ignore-scripts" {
            self.ignore_scripts = parse_bool(value);
            return;
        }
        if key == "inject-workspace-packages" {
            self.inject_workspace_packages = parse_bool(value);
            return;
        }
        if key == "maxsockets" {
            self.maxsockets = value.parse().ok();
            return;
        }
        if key == "max-sockets" {
            self.max_sockets = value.parse().ok();
            return;
        }
        if key == "minimum-release-age" {
            self.minimum_release_age = value.parse().ok();
            return;
        }
        if key == "minimum-release-age-exclude" {
            // nopt collects a repeated key it has no type for into a list,
            // and pnpm re-parses the `--config.` tokens without any types.
            self.minimum_release_age_exclude.get_or_insert_default().push(value.to_string());
            return;
        }
        if key == "minimum-release-age-ignore-missing-time" {
            self.minimum_release_age_ignore_missing_time = parse_bool(value);
            return;
        }
        if key == "minimum-release-age-strict" {
            self.minimum_release_age_strict = parse_bool(value);
            return;
        }
        if key == "modules-dir" {
            self.modules_dir = Some(value.to_string());
            return;
        }
        if key == "node-linker" {
            self.node_linker = parse_enum(value);
            return;
        }
        if key == "package-import-method" {
            self.package_import_method = parse_enum(value);
            return;
        }
        if key == "public-hoist-pattern" {
            self.public_hoist_pattern.get_or_insert_default().push(value.to_string());
            return;
        }
        if key == "pm-on-fail" {
            self.pm_on_fail = parse_enum(value);
            return;
        }
        if key == "runtime-on-fail" {
            self.runtime_on_fail = parse_enum(value);
            return;
        }
        if key == "shared-workspace-lockfile" {
            self.shared_workspace_lockfile = parse_bool(value);
            return;
        }
        if key == "verify-deps-before-run" {
            self.verify_deps_before_run = value.parse().ok();
            return;
        }
        if key == "trust-policy" {
            self.trust_policy = parse_enum(value);
            return;
        }
        if key == "trust-policy-exclude" {
            self.trust_policy_exclude.get_or_insert_default().push(value.to_string());
            return;
        }
        if key == "trust-policy-ignore-after" {
            self.trust_policy_ignore_after = value.parse().ok();
            return;
        }
        if key == "virtual-store-dir" {
            self.virtual_store_dir = Some(value.to_string());
            return;
        }
        if let Some(scope) = scoped_registry_key(key) {
            self.registries.insert(scope.to_owned(), normalize_registry_url(value));
        }
    }

    /// Layer the CLI overrides on top of a [`Config`] that has already
    /// been built from defaults, `.npmrc`, and `pnpm-workspace.yaml`.
    /// Mirrors pnpm 11's "CLI > yaml > .npmrc > defaults" precedence.
    ///
    /// `dir` is the canonicalized `--dir`, the fallback base for a
    /// relative path-valued setting outside a workspace.
    pub fn apply(&self, config: &mut Config, dir: &Path) {
        config.apply_proxy_cli_overrides(
            self.https_proxy.as_deref(),
            self.http_proxy.as_deref(),
            self.no_proxy.as_deref(),
        );
        if let Some(value) = self.allow_unused_patches {
            config.allow_unused_patches = value;
            config.explicit_settings.insert("allowUnusedPatches".to_string(), value.into());
        }
        if let Some(value) = self.bail {
            config.bail = value;
        }
        if let Some(value) = self.ci {
            config.ci = value;
        }
        if let Some(value) = self.color {
            config.color = value;
        }
        if let Some(value) = self.embed_readme {
            config.embed_readme = value;
        }
        if let Some(value) = self.ignore_workspace_root_check {
            config.ignore_workspace_root_check = value;
        }
        if let Some(value) = self.optional {
            config.optional = value;
        }
        if let Some(value) = self.package_lock {
            config.package_lock = value;
            if self.lockfile.is_none() && !config.explicit_settings.contains_key("lockfile") {
                config.lockfile = value;
            }
        }
        if let Some(value) = self.lockfile {
            config.lockfile = value;
            config.explicit_settings.insert("lockfile".to_string(), value.into());
        }
        if let Some(value) = self.pending {
            config.pending = value;
        }
        if let Some(value) = self.recursive_install {
            config.recursive_install = value;
        }
        if let Some(value) = self.reverse {
            config.reverse = value;
        }
        // Ahead of the hoist settings below: a `--no-virtual-store-only`
        // gets the lower layers' patterns back first, and a pattern on the
        // same command line then replaces them.
        if let Some(value) = self.virtual_store_only {
            config.virtual_store_only = value;
            config.explicit_settings.insert("virtualStoreOnly".to_string(), value.into());
            if value {
                config.apply_virtual_store_only_derivation();
            } else {
                config.restore_hoist_patterns_after_virtual_store_only();
            }
        }
        if let Some(value) = self.shamefully_hoist {
            config.shamefully_hoist = value;
            config.explicit_settings.insert("shamefullyHoist".to_string(), value.into());
        }
        if let Some(value) = self.hoist {
            config.hoist = value;
            config.explicit_settings.insert("hoist".to_string(), value.into());
        }
        if let Some(value) = &self.hoist_pattern {
            config.hoist_pattern = Some(value.clone());
            config.explicit_settings.insert("hoistPattern".to_string(), value.as_slice().into());
        }
        if let Some(value) = &self.public_hoist_pattern {
            config.public_hoist_pattern = Some(value.clone());
            config
                .explicit_settings
                .insert("publicHoistPattern".to_string(), value.as_slice().into());
        }
        if self.shamefully_hoist.is_some()
            || self.hoist.is_some()
            || self.hoist_pattern.is_some()
            || self.public_hoist_pattern.is_some()
        {
            // `hoist: false` nullifies the private pattern whichever layer
            // supplied either, so the two derivations below re-run over the
            // command line's contribution the way `WorkspaceSettings::apply_to`
            // runs them over yaml's.
            if !config.hoist {
                config.hoist_pattern = None;
            }
            config.apply_shamefully_hoist_derivation();
            config.apply_virtual_store_only_derivation();
        }
        if let Some(value) = self.shell_emulator {
            config.shell_emulator = value;
        }
        if let Some(value) = self.skip_manifest_obfuscation {
            config.skip_manifest_obfuscation = value;
        }
        if let Some(value) = self.sort {
            config.sort = value;
        }
        if let Some(value) = self.use_beta_cli {
            config.use_beta_cli = value;
        }
        if let Some(registry) = &self.registry {
            apply_registry_override(config, registry);
        }
        if let Some(scope) = &self.scope {
            config.scope = Some(scope.clone());
        }
        for (scope, registry) in &self.registries {
            config.registries_by_scope.insert(scope.clone(), registry.clone());
            config.package_manager_bootstrap.registries.insert(scope.clone(), registry.clone());
        }
        if let Some(value) = self.deploy_all_files {
            config.deploy_all_files = value;
        }
        if let Some(value) = self.force_legacy_deploy {
            config.force_legacy_deploy = value;
        }
        // `pnpm config get ignore-scripts` answers from the explicitly-set
        // settings, so a CLI-set value has to be recorded there to be
        // reported as set while it suppresses the scripts.
        if let Some(value) = self.ignore_scripts {
            config.ignore_scripts = value;
            config.explicit_settings.insert("ignoreScripts".to_string(), value.into());
        }
        if let Some(value) = self.inject_workspace_packages {
            config.inject_workspace_packages = value;
        }
        // npm's spelling first, so the canonical one wins when a single
        // command line carries both.
        if let Some(value) = self.maxsockets {
            config.max_sockets = Some(value);
        }
        if let Some(value) = self.max_sockets {
            config.max_sockets = Some(value);
        }
        // pnpm seeds `explicitlySetKeys` from the command line as well as
        // from the config files, and the workspace state reads it back to
        // decide whether `minimumReleaseAgeStrict` defaults to true.
        if let Some(value) = self.minimum_release_age {
            config.minimum_release_age = Some(value);
            config.explicit_settings.insert("minimumReleaseAge".to_string(), value.into());
        }
        if let Some(value) = &self.minimum_release_age_exclude {
            config.minimum_release_age_exclude = Some(value.clone());
            config
                .explicit_settings
                .insert("minimumReleaseAgeExclude".to_string(), value.as_slice().into());
        }
        if let Some(value) = self.minimum_release_age_ignore_missing_time {
            config.minimum_release_age_ignore_missing_time = value;
            config
                .explicit_settings
                .insert("minimumReleaseAgeIgnoreMissingTime".to_string(), value.into());
        }
        if let Some(value) = self.minimum_release_age_strict {
            config.minimum_release_age_strict = Some(value);
            config.explicit_settings.insert("minimumReleaseAgeStrict".to_string(), value.into());
        }
        if let Some(value) = self.node_linker {
            config.node_linker = value;
            // A CLI-selected hoisted linker turns the default on just
            // like a yaml-selected one — pnpm merges CLI options before
            // its `nodeLinker` switch, so the derivation must see this
            // override too.
            config.apply_prefer_symlinked_executables_derivation();
        }
        if let Some(value) = self.pm_on_fail {
            config.pm_on_fail = Some(value);
        }
        if let Some(value) = self.runtime_on_fail {
            config.runtime_on_fail = Some(value);
        }
        if let Some(value) = self.shared_workspace_lockfile {
            config.shared_workspace_lockfile = value;
            config.explicit_settings.insert("sharedWorkspaceLockfile".to_string(), value.into());
        }
        // The `pnpm_config_verify_deps_before_run` env var outranks even
        // the CLI for this one key (pnpm's config reader applies it after
        // every other layer): pnpm stamps `false` into every spawned
        // script's env, and a nested `pnpm run` inside a script must see
        // the check disabled no matter what flags the outer invocation
        // carried, or the spawned install's lifecycle scripts would
        // re-enter the check (pnpm/pnpm#10060).
        if let Some(value) = self.verify_deps_before_run
            && !verify_deps_env_is_set()
        {
            config.verify_deps_before_run = value;
        }
        // `pnpm config get <setting>` answers from the explicitly-set
        // settings, and pnpm seeds those from the command line as well as
        // from the config files, so each override below records itself
        // there alongside the value it resolves.
        if let Some(value) = self.package_import_method {
            config.package_import_method = value;
            config
                .explicit_settings
                .insert("packageImportMethod".to_string(), setting_value(value));
        }
        if let Some(value) = self.child_concurrency {
            config.child_concurrency = resolve_child_concurrency(Some(value));
            config.explicit_settings.insert("childConcurrency".to_string(), value.into());
        }
        if let Some(value) = self.strict_peer_dependencies {
            config.strict_peer_dependencies = value;
            config.explicit_settings.insert("strictPeerDependencies".to_string(), value.into());
        }
        if let Some(value) = self.side_effects_cache {
            config.apply_side_effects_cache_shorthand(value);
            config.explicit_settings.insert("sideEffectsCache".to_string(), value.into());
        }
        if let Some(value) = self.side_effects_cache_readonly {
            config.side_effects_cache_readonly = value;
            config.explicit_settings.insert("sideEffectsCacheReadonly".to_string(), value.into());
        }
        if let Some(value) = self.optimistic_repeat_install {
            config.optimistic_repeat_install = value;
            config.explicit_settings.insert("optimisticRepeatInstall".to_string(), value.into());
        }
        if let Some(value) = self.trust_lockfile {
            config.trust_lockfile = value;
            config.explicit_settings.insert("trustLockfile".to_string(), value.into());
        }
        if let Some(value) = self.trust_policy {
            config.trust_policy = value;
            config.explicit_settings.insert("trustPolicy".to_string(), setting_value(value));
        }
        if let Some(value) = &self.trust_policy_exclude {
            config.trust_policy_exclude = Some(value.clone());
            config
                .explicit_settings
                .insert("trustPolicyExclude".to_string(), value.as_slice().into());
        }
        if let Some(value) = self.trust_policy_ignore_after {
            config.trust_policy_ignore_after = Some(value);
            config.explicit_settings.insert("trustPolicyIgnoreAfter".to_string(), value.into());
        }
        if let Some(value) = self.unsafe_perm {
            config.unsafe_perm = value;
            config.explicit_settings.insert("unsafePerm".to_string(), value.into());
        }
        if let Some(value) = self.dangerously_allow_all_builds {
            config.dangerously_allow_all_builds = value;
            config.explicit_settings.insert("dangerouslyAllowAllBuilds".to_string(), value.into());
        }
        if let Some(value) = self.engine_strict {
            config.engine_strict = value;
            config.explicit_settings.insert("engineStrict".to_string(), value.into());
        }
        if let Some(value) = self.frozen_store {
            config.frozen_store = value;
            config.explicit_settings.insert("frozenStore".to_string(), value.into());
        }
        if let Some(value) = self.ignore_pnpmfile {
            config.ignore_pnpmfile = value;
            config.explicit_settings.insert("ignorePnpmfile".to_string(), value.into());
        }
        if let Some(value) = self.link_workspace_packages {
            config.link_workspace_packages = value;
            config
                .explicit_settings
                .insert("linkWorkspacePackages".to_string(), setting_value(value));
        }
        if let Some(value) = self.lockfile_include_tarball_url {
            config.lockfile_include_tarball_url = value;
            config.explicit_settings.insert("lockfileIncludeTarballUrl".to_string(), value.into());
        }
        if let Some(value) = self.merge_git_branch_lockfiles {
            config.merge_git_branch_lockfiles = value;
            config.explicit_settings.insert("mergeGitBranchLockfiles".to_string(), value.into());
        }
        if let Some(value) = self.node_experimental_package_map {
            config.node_experimental_package_map = value;
            config.explicit_settings.insert("nodeExperimentalPackageMap".to_string(), value.into());
        }
        if let Some(value) = self.offline {
            config.offline = value;
            config.explicit_settings.insert("offline".to_string(), value.into());
        }
        if let Some(value) = self.prefer_frozen_lockfile {
            config.prefer_frozen_lockfile = value;
            config.explicit_settings.insert("preferFrozenLockfile".to_string(), value.into());
        }
        if let Some(value) = self.prefer_offline {
            config.prefer_offline = value;
            config.explicit_settings.insert("preferOffline".to_string(), value.into());
        }
        if let Some(value) = self.save_workspace_protocol {
            config.save_workspace_protocol = value;
            config
                .explicit_settings
                .insert("saveWorkspaceProtocol".to_string(), setting_value(value));
        }
        if let Some(value) = self.verify_store_integrity {
            config.verify_store_integrity = value;
            config.explicit_settings.insert("verifyStoreIntegrity".to_string(), value.into());
        }
        if let Some(value) = self.global_dir.as_deref().filter(|value| !value.is_empty()) {
            let global_dir = lexical_normalize(&dir.join(value));
            config.global_pkg_dir = Some(global_dir.join(GLOBAL_LAYOUT_VERSION));
            config.global_dir = Some(global_dir);
            config.explicit_settings.insert("globalDir".to_string(), value.into());
        }
        self.apply_lockfile_anchored_paths(config, dir);
    }

    /// Re-anchor the root `node_modules` and the virtual store onto a
    /// command-line `--modules-dir` / `--virtual-store-dir`.
    ///
    /// Both reach [`Config::explicit_settings`] as the raw spelling, which
    /// is what [`Config::anchor_lockfile_paths`] resolves — so a later
    /// `--lockfile-dir` pin moves the CLI-set paths along with it.
    fn apply_lockfile_anchored_paths(&self, config: &mut Config, dir: &Path) {
        let raw_settings = [
            ("modulesDir", self.modules_dir.as_deref()),
            ("virtualStoreDir", self.virtual_store_dir.as_deref()),
        ];
        let mut anchored = false;
        for (setting, value) in raw_settings {
            if let Some(value) = value {
                config.explicit_settings.insert(setting.to_string(), value.into());
                anchored = true;
            }
        }
        if !anchored {
            return;
        }
        let anchor = config
            .lockfile_dir
            .clone()
            .or_else(|| config.workspace_dir.clone())
            .unwrap_or_else(|| dir.to_path_buf());
        config.anchor_lockfile_paths(&anchor);
        let virtual_store_dir_explicit = config.explicit_settings.contains_key("virtualStoreDir");
        let global_virtual_store_dir_explicit =
            config.explicit_settings.contains_key("globalVirtualStoreDir");
        config.apply_global_virtual_store_derivation(
            virtual_store_dir_explicit,
            global_virtual_store_dir_explicit,
        );
    }
}

/// Presence-only, like pnpm's `!= null` check: an empty value still
/// overrides (it disables the gate on the env-overlay side).
fn verify_deps_env_is_set() -> bool {
    ["PNPM_CONFIG_VERIFY_DEPS_BEFORE_RUN", pnpm_executor::VERIFY_DEPS_BEFORE_RUN_ENV]
        .iter()
        .any(|name| std::env::var(name).is_ok())
}

enum ConfigToken<'a> {
    WellFormed {
        key: &'a str,
        value: &'a str,
    },
    /// A bare `--<setting>` for a boolean setting: `true`, unless the next
    /// argv token spells a boolean and is claimed as its value.
    BooleanFollows(&'static str),
    /// A bare `--<setting>` for a value-taking setting: the next argv
    /// token is its value.
    ValueFollows(&'static str),
    Malformed,
    NotOurs,
}

/// Decide whether an argv token belongs to the `--config.<key>=<value>`
/// family or is one of the [`BARE_SETTING_FLAGS`]. Everything with a
/// `--config.` prefix is claimed, so a typo like `--config.foo` never
/// escapes into clap's "unexpected argument" path; every other token is
/// returned untouched.
///
/// `claimed_by_command` names the options the invoked command declares
/// itself, which win over the setting of the same name — see
/// [`subcommand_option_names`].
///
/// A setting given a value it does not take — a misspelled boolean, an
/// unknown `--trust-policy`, a non-numeric `--child-concurrency` — is
/// left for clap, which reports it as an unexpected argument. Dropping
/// it instead would leave the install running under a setting the user
/// believes they changed, which for a supply-chain setting like
/// `trustPolicy` means failing open.
///
/// [`subcommand_option_names`]: crate::parse_boundary::subcommand_option_names
fn classify<'a>(arg: &'a OsStr, claimed_by_command: &HashSet<&str>) -> ConfigToken<'a> {
    let setting =
        |key: &str| named_bare_setting_flag(key).filter(|_| !claimed_by_command.contains(key));
    let Some(arg) = arg.to_str() else {
        return ConfigToken::NotOurs;
    };
    if let Some(rest) = arg.strip_prefix("--config.") {
        let Some((key, value)) = rest.split_once('=') else {
            return ConfigToken::Malformed;
        };
        if key.is_empty() {
            return ConfigToken::Malformed;
        }
        // The dotted spelling names a setting outright, so a command
        // option of the same name never shadows it.
        if !setting_takes(key, value) {
            return ConfigToken::NotOurs;
        }
        return ConfigToken::WellFormed { key, value };
    }
    let Some(flag) = arg.strip_prefix("--") else {
        return ConfigToken::NotOurs;
    };
    if let Some(negated) = flag.strip_prefix("no-")
        && let Some((key, SettingArity::Boolean | SettingArity::BooleanOr { .. })) =
            setting(negated)
    {
        return ConfigToken::WellFormed { key, value: "false" };
    }
    let Some((key, value)) = flag.split_once('=') else {
        return match setting(flag) {
            Some((key, SettingArity::Boolean | SettingArity::BooleanOr { .. })) => {
                ConfigToken::BooleanFollows(key)
            }
            Some((key, _)) => ConfigToken::ValueFollows(key),
            None => ConfigToken::NotOurs,
        };
    };
    match setting(key) {
        Some((_, SettingArity::BooleanOr { bare_keyword: false, .. }))
            if parse_bool(value).is_none() =>
        {
            ConfigToken::NotOurs
        }
        Some(_) if !setting_takes(key, value) => ConfigToken::NotOurs,
        Some((key, _)) => ConfigToken::WellFormed { key, value },
        None => ConfigToken::NotOurs,
    }
}

/// How much of argv a bare `--<setting>` flag claims, and which values it
/// takes there. A setting whose value is a list repeats the flag, one
/// value per occurrence.
#[derive(Debug, Clone, Copy)]
enum SettingArity {
    /// `--<setting>`, `--no-<setting>`, `--<setting>=<bool>`, and
    /// `--<setting> <bool>`.
    Boolean,
    /// Every [`Boolean`] spelling, plus a keyword `takes` accepts, for a
    /// setting whose type is a boolean or a keyword. Only a boolean is
    /// claimed from the token after the flag.
    ///
    /// `bare_keyword` says whether the keyword is spellable as
    /// `--<setting>=<keyword>`, which follows the `nopt` type pnpm gives
    /// the setting: `linkWorkspacePackages` is `[Boolean, 'deep']`, so
    /// `--link-workspace-packages=deep` parses; `saveWorkspaceProtocol` is
    /// `Boolean` alone, so `rolling` reaches it only through the untyped
    /// `--config.` form.
    ///
    /// [`Boolean`]: Self::Boolean
    BooleanOr { takes: fn(&str) -> bool, bare_keyword: bool },
    /// A path, a glob pattern, or another free-form value: every spelling
    /// is one the setting takes, so the token after the flag is claimed
    /// only when it cannot be anything else — see [`claims_as_value`].
    Text,
    /// A value the carried predicate accepts, and only such a value.
    Parsed(fn(&str) -> bool),
}

/// Settings pnpm accepts as a bare `--<setting>` command-line flag, with
/// the values each takes.
///
/// pnpm declares a `nopt` type for every setting, which makes all of them
/// spellable on the command line; pacquet declares a clap flag for only a
/// subset, so the rest are recognized here and layered onto [`Config`]
/// exactly like a `--config.<setting>=<value>` token
/// ([pnpm/pnpm#14281](https://github.com/pnpm/pnpm/issues/14281)). A
/// setting the invoked command declares as its own option is left for
/// clap; a setting that collides with a *global* option would be claimed
/// on every command line and so must not appear here at all.
const BARE_SETTING_FLAGS: [(&str, SettingArity); 39] = [
    ("allow-unused-patches", SettingArity::Boolean),
    ("child-concurrency", SettingArity::Parsed(is_i32)),
    ("dangerously-allow-all-builds", SettingArity::Boolean),
    ("engine-strict", SettingArity::Boolean),
    ("force-legacy-deploy", SettingArity::Boolean),
    ("frozen-store", SettingArity::Boolean),
    ("global-dir", SettingArity::Text),
    ("hoist", SettingArity::Boolean),
    ("hoist-pattern", SettingArity::Text),
    ("ignore-pnpmfile", SettingArity::Boolean),
    ("ignore-scripts", SettingArity::Boolean),
    (
        "link-workspace-packages",
        SettingArity::BooleanOr { takes: is_enum::<LinkWorkspacePackages>, bare_keyword: true },
    ),
    ("lockfile", SettingArity::Boolean),
    ("lockfile-include-tarball-url", SettingArity::Boolean),
    ("merge-git-branch-lockfiles", SettingArity::Boolean),
    ("modules-dir", SettingArity::Text),
    ("node-experimental-package-map", SettingArity::Boolean),
    ("offline", SettingArity::Boolean),
    ("optimistic-repeat-install", SettingArity::Boolean),
    ("package-import-method", SettingArity::Parsed(is_enum::<PackageImportMethod>)),
    ("pm-on-fail", SettingArity::Parsed(is_enum::<PmOnFail>)),
    ("prefer-frozen-lockfile", SettingArity::Boolean),
    ("prefer-offline", SettingArity::Boolean),
    ("public-hoist-pattern", SettingArity::Text),
    ("runtime-on-fail", SettingArity::Parsed(is_enum::<RuntimeOnFail>)),
    (
        "save-workspace-protocol",
        SettingArity::BooleanOr { takes: is_enum::<SaveWorkspaceProtocol>, bare_keyword: false },
    ),
    ("shamefully-hoist", SettingArity::Boolean),
    ("shared-workspace-lockfile", SettingArity::Boolean),
    ("side-effects-cache", SettingArity::Boolean),
    ("side-effects-cache-readonly", SettingArity::Boolean),
    ("strict-peer-dependencies", SettingArity::Boolean),
    ("trust-lockfile", SettingArity::Boolean),
    ("trust-policy", SettingArity::Parsed(is_enum::<TrustPolicy>)),
    ("trust-policy-exclude", SettingArity::Text),
    ("trust-policy-ignore-after", SettingArity::Parsed(is_u64)),
    ("unsafe-perm", SettingArity::Boolean),
    ("verify-store-integrity", SettingArity::Boolean),
    ("virtual-store-dir", SettingArity::Text),
    ("virtual-store-only", SettingArity::Boolean),
];

fn is_i32(value: &str) -> bool {
    value.parse::<i32>().is_ok()
}

fn is_u64(value: &str) -> bool {
    value.parse::<u64>().is_ok()
}

fn is_enum<Value: serde::de::DeserializeOwned>(value: &str) -> bool {
    parse_enum::<Value>(value).is_some()
}

fn named_bare_setting_flag(key: &str) -> Option<(&'static str, SettingArity)> {
    BARE_SETTING_FLAGS.into_iter().find(|&(name, _)| name == key)
}

/// Whether `value` is a spelling the `key` setting takes. `true` for a
/// key outside [`BARE_SETTING_FLAGS`], which keeps the `--config.<key>`
/// tolerance for the settings pacquet has not ported.
fn setting_takes(key: &str, value: &str) -> bool {
    match named_bare_setting_flag(key) {
        Some((_, SettingArity::Boolean)) => parse_bool(value).is_some(),
        Some((_, SettingArity::BooleanOr { takes, .. })) => {
            parse_bool(value).is_some() || takes(value)
        }
        Some((_, SettingArity::Text)) => true,
        Some((_, SettingArity::Parsed(takes))) => takes(value),
        None => true,
    }
}

/// Whether the `key` setting claims `token` — the argv token after its
/// flag — as its value.
///
/// A free-form setting refuses one that opens with `-`: that token is the
/// `--` separator, another flag, or a short option, and claiming it would
/// drop the separator or point a path setting at a directory named `--`.
/// A parsed setting decides on its own terms instead, which is what lets
/// `--child-concurrency -1` mean "every core but one".
fn claims_as_value(key: &str, token: &str) -> bool {
    match named_bare_setting_flag(key) {
        Some((_, SettingArity::Boolean | SettingArity::BooleanOr { .. })) => {
            is_boolean_value(token)
        }
        Some((_, SettingArity::Text)) => !token.starts_with('-'),
        Some((_, SettingArity::Parsed(takes))) => takes(token),
        None => false,
    }
}

/// Whether a token spells a boolean a bare `--<setting>` flag claims as
/// its value — the same spellings the `--<setting>=<bool>` form takes,
/// so the two agree. Only a boolean is claimed, which is what leaves
/// `pnpm --shamefully-hoist install` its command.
fn is_boolean_value(token: &str) -> bool {
    parse_bool(token).is_some()
}

/// Whether a bare boolean setting flag claims `next` as its value.
///
/// The scan that has to find the subcommand applies this ahead of clap's
/// own arity: nothing else on a command line is spelled `true` /
/// `false`, so stepping over that token is right whichever reading of
/// the flag ends up applying — a command's option of the same name, or
/// the setting. Without it the two readings disagree on width for a name
/// that is both, and `pnpm --lockfile true --config.registry=… install`
/// loses everything past the boolean to the script fallback.
pub(crate) fn bare_boolean_setting_claims(flag: &str, next: Option<&str>) -> bool {
    matches!(
        named_bare_setting_flag(flag),
        Some((_, SettingArity::Boolean | SettingArity::BooleanOr { .. })),
    ) && next.is_some_and(is_boolean_value)
}

/// How many argv slots a bare `--<setting>` flag occupies, given the
/// token after it — for the scan that has to find the subcommand before
/// the settings are stripped. `1` for a token that is not one of the
/// [`BARE_SETTING_FLAGS`], or one whose value is missing, which
/// [`ConfigOverrides::extract`] hands to clap intact.
pub(crate) fn bare_setting_flag_width(flag: &str, next: Option<&str>) -> usize {
    1 + usize::from(next.is_some_and(|next| claims_as_value(flag, next)))
}

fn scoped_registry_key(key: &str) -> Option<&str> {
    key.strip_suffix(":registry")
        .filter(|scope| scope.starts_with('@') && scope.len() > 1 && !scope.contains('/'))
}

/// Layer a registry URL (the universal `--registry` flag or a
/// `--config.registry=<url>` override) onto `config`, setting the default
/// registry everywhere it is read: the resolved registry, the `default`
/// entry of the named-registry map, and the package-manager bootstrap
/// copies of both. The URL is normalized to a trailing slash first, so an
/// already-normalized override applies idempotently.
pub(crate) fn apply_registry_override(config: &mut Config, registry: &str) {
    let registry = normalize_registry_url(registry);
    config.registry.clone_from(&registry);
    config.registries_by_scope.insert("default".to_string(), registry.clone());
    config.package_manager_bootstrap.registry.clone_from(&registry);
    config.package_manager_bootstrap.registries.insert("default".to_string(), registry);
}

fn normalize_registry_url(registry: &str) -> String {
    if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") }
}

fn parse_enum<Value: serde::de::DeserializeOwned>(value: &str) -> Option<Value> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).ok()
}

/// A setting whose type is a boolean or a keyword, from either spelling.
fn parse_bool_or_enum<Value: serde::de::DeserializeOwned>(value: &str) -> Option<Value> {
    match parse_bool(value) {
        Some(boolean) => serde_json::from_value(serde_json::Value::Bool(boolean)).ok(),
        None => parse_enum(value),
    }
}

/// An enum setting's kebab-case spelling, for [`Config::explicit_settings`]
/// — the form the config files and `pnpm config get` use.
fn setting_value<Value: serde::Serialize>(value: Value) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
