pub(crate) use apply::{
    apply_registry_override, apply_state_dir_override, apply_store_dir_override,
};
pub(crate) use tokens::{bare_boolean_setting_claims, bare_setting_flag_width, parse_bool};

use apply::normalize_registry_url;

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
use tokens::{
    ConfigToken, claims_as_value, classify, is_forwarded, parse_bool_or_enum, parse_enum,
    scoped_registry_key, setting_value, verify_deps_env_is_set,
};

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

/// Copy each override that the command line set onto the config.
macro_rules! copy_overrides {
    ($self:ident, $config:ident, $($field:ident),* $(,)?) => {
        $(
            if let Some(value) = $self.$field {
                $config.$field = value;
            }
        )*
    };
}

/// Like [`copy_overrides!`], and record each setting as explicitly set.
/// `pnpm config get <setting>` answers from the explicitly-set settings,
/// and pnpm seeds those from the command line as well as from the config
/// files, so a command-line override has to leave its mark there too.
macro_rules! record_overrides {
    ($self:ident, $config:ident, $($field:ident => $key:literal),* $(,)?) => {
        $(
            if let Some(value) = $self.$field {
                $config.$field = value;
                $config.explicit_settings.insert($key.to_string(), value.into());
            }
        )*
    };
}

/// [`record_overrides!`] for a setting whose value is an enum, which
/// renders back to its config spelling through [`setting_value`].
macro_rules! record_enum_overrides {
    ($self:ident, $config:ident, $($field:ident => $key:literal),* $(,)?) => {
        $(
            if let Some(value) = $self.$field {
                $config.$field = value;
                $config.explicit_settings.insert($key.to_string(), setting_value(value));
            }
        )*
    };
}

/// [`record_overrides!`] for a setting the command line accumulates into a
/// list.
macro_rules! record_list_overrides {
    ($self:ident, $config:ident, $($field:ident => $key:literal),* $(,)?) => {
        $(
            if let Some(value) = &$self.$field {
                $config.$field = Some(value.clone());
                $config.explicit_settings.insert($key.to_string(), value.as_slice().into());
            }
        )*
    };
}

impl ConfigOverrides {
    /// Pull `--config.<key>=<value>` tokens and [`BARE_SETTING_FLAGS`](tokens::BARE_SETTING_FLAGS)
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
        let mut overrides = Self::default();
        let mut remaining = Vec::new();
        let mut argv = argv.into_iter().enumerate().peekable();
        while let Some((index, arg)) = argv.next() {
            if is_forwarded(passthrough_from, index) {
                remaining.push(arg);
                continue;
            }
            // The token after a `--<setting> <value>` pair's flag, when the
            // setting claims it — see [`claims_as_value`]. `None` when the
            // flag ends argv, the token is already the child's, or it is
            // not a value the setting takes, all of which leave the
            // valueless flag for clap to report.
            let mut following = |key: &str| {
                let next_token = argv
                    .peek()
                    .filter(|&&(index, _)| !is_forwarded(passthrough_from, index))
                    .and_then(|(_, token)| token.to_str());
                let value =
                    next_token.filter(|token| claims_as_value(key, token)).map(str::to_owned)?;
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
        self.set_boolean_install_option(key, value);
        self.set_boolean_execution_option(key, value);
        self.set_network_option(key, value);
        self.set_dependency_policy_option(key, value);
        self.set_layout_option(key, value);
        self.set_install_execution_option(key, value);
        if let Some(scope) = scoped_registry_key(key) {
            self.registries.insert(scope.to_owned(), normalize_registry_url(value));
        }
    }

    fn set_boolean_install_option(&mut self, key: &str, value: &str) {
        match key {
            "allow-unused-patches" => self.allow_unused_patches = parse_bool(value),
            "dangerously-allow-all-builds" => {
                self.dangerously_allow_all_builds = parse_bool(value);
            }
            "engine-strict" => self.engine_strict = parse_bool(value),
            "frozen-store" => self.frozen_store = parse_bool(value),
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
            "offline" => self.offline = parse_bool(value),
            "optimistic-repeat-install" => self.optimistic_repeat_install = parse_bool(value),
            "optional" => self.optional = parse_bool(value),
            "package-lock" => self.package_lock = parse_bool(value),
            "prefer-frozen-lockfile" => self.prefer_frozen_lockfile = parse_bool(value),
            "prefer-offline" => self.prefer_offline = parse_bool(value),
            "save-workspace-protocol" => {
                self.save_workspace_protocol = parse_bool_or_enum(value);
            }
            "shamefully-hoist" => self.shamefully_hoist = parse_bool(value),
            "side-effects-cache" => self.side_effects_cache = parse_bool(value),
            "side-effects-cache-readonly" => {
                self.side_effects_cache_readonly = parse_bool(value);
            }
            "strict-peer-dependencies" => self.strict_peer_dependencies = parse_bool(value),
            "trust-lockfile" => self.trust_lockfile = parse_bool(value),
            "verify-store-integrity" => self.verify_store_integrity = parse_bool(value),
            "virtual-store-only" => self.virtual_store_only = parse_bool(value),
            _ => {}
        }
    }

    fn set_boolean_execution_option(&mut self, key: &str, value: &str) {
        match key {
            "bail" => self.bail = parse_bool(value),
            "ci" => self.ci = parse_bool(value),
            "color" => {
                self.color = parse_bool(value)
                    .map(|enabled| if enabled { ColorMode::Always } else { ColorMode::Never })
                    .or_else(|| parse_enum(value));
            }
            "embed-readme" => self.embed_readme = parse_bool(value),
            "ignore-workspace-root-check" => {
                self.ignore_workspace_root_check = parse_bool(value);
            }
            "node-experimental-package-map" => {
                self.node_experimental_package_map = parse_bool(value);
            }
            "pending" => self.pending = parse_bool(value),
            "recursive-install" => self.recursive_install = parse_bool(value),
            "reverse" => self.reverse = parse_bool(value),
            "shell-emulator" => self.shell_emulator = parse_bool(value),
            "skip-manifest-obfuscation" => {
                self.skip_manifest_obfuscation = parse_bool(value);
            }
            "sort" => self.sort = parse_bool(value),
            "unsafe-perm" => self.unsafe_perm = parse_bool(value),
            "use-beta-cli" => self.use_beta_cli = parse_bool(value),
            _ => {}
        }
    }

    fn set_network_option(&mut self, key: &str, value: &str) {
        match key {
            "registry" => {
                self.registry = Some(normalize_registry_url(value));
            }
            "scope" => {
                self.scope = Some(value.to_string());
            }
            "https-proxy" => {
                self.https_proxy = Some(value.to_string());
            }
            "http-proxy" => {
                self.http_proxy = Some(value.to_string());
            }
            "no-proxy" => {
                self.no_proxy = Some(value.to_string());
            }
            "maxsockets" => {
                self.maxsockets = value.parse().ok();
            }
            "max-sockets" => {
                self.max_sockets = value.parse().ok();
            }
            _ => {}
        }
    }

    fn set_dependency_policy_option(&mut self, key: &str, value: &str) {
        match key {
            "minimum-release-age" => {
                self.minimum_release_age = value.parse().ok();
            }
            "minimum-release-age-exclude" => {
                // nopt collects a repeated key it has no type for into a list,
                // and pnpm re-parses the `--config.` tokens without any types.
                self.minimum_release_age_exclude.get_or_insert_default().push(value.to_string());
            }
            "minimum-release-age-ignore-missing-time" => {
                self.minimum_release_age_ignore_missing_time = parse_bool(value);
            }
            "minimum-release-age-strict" => {
                self.minimum_release_age_strict = parse_bool(value);
            }
            "pm-on-fail" => {
                self.pm_on_fail = parse_enum(value);
            }
            "runtime-on-fail" => {
                self.runtime_on_fail = parse_enum(value);
            }
            "verify-deps-before-run" => {
                self.verify_deps_before_run = value.parse().ok();
            }
            "trust-policy" => {
                self.trust_policy = parse_enum(value);
            }
            "trust-policy-exclude" => {
                self.trust_policy_exclude.get_or_insert_default().push(value.to_string());
            }
            "trust-policy-ignore-after" => {
                self.trust_policy_ignore_after = value.parse().ok();
            }
            _ => {}
        }
    }

    fn set_layout_option(&mut self, key: &str, value: &str) {
        match key {
            "global-dir" => {
                self.global_dir = Some(value.to_string());
            }
            "hoist-pattern" => {
                self.hoist_pattern.get_or_insert_default().push(value.to_string());
            }
            "modules-dir" => {
                self.modules_dir = Some(value.to_string());
            }
            "node-linker" => {
                self.node_linker = parse_enum(value);
            }
            "public-hoist-pattern" => {
                self.public_hoist_pattern.get_or_insert_default().push(value.to_string());
            }
            "virtual-store-dir" => {
                self.virtual_store_dir = Some(value.to_string());
            }
            _ => {}
        }
    }

    fn set_install_execution_option(&mut self, key: &str, value: &str) {
        match key {
            "child-concurrency" => {
                self.child_concurrency = value.parse().ok();
            }
            "deploy-all-files" => {
                self.deploy_all_files = parse_bool(value);
            }
            "force-legacy-deploy" => {
                self.force_legacy_deploy = parse_bool(value);
            }
            "ignore-scripts" => {
                self.ignore_scripts = parse_bool(value);
            }
            "inject-workspace-packages" => {
                self.inject_workspace_packages = parse_bool(value);
            }
            "package-import-method" => {
                self.package_import_method = parse_enum(value);
            }
            "shared-workspace-lockfile" => {
                self.shared_workspace_lockfile = parse_bool(value);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;

mod apply;

mod tokens;
