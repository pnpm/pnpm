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
#[cfg_attr(
    dylint_lib = "perfectionist",
    expect(
        perfectionist::too_many_struct_fields,
        reason = "The fields mirror pnpm configuration keys supplied by command-line overrides."
    )
)]
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
        let mut argv = argv
            .into_iter()
            .enumerate()
            .peekable();
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
            let mut following = |key: &str| take_following_value(&mut argv, passthrough_from, key);
            if let Some(arg) = overrides.apply_token(arg, &claimed_by_command, &mut following) {
                remaining.push(arg);
            }
        }
        (overrides, remaining)
    }
}

#[cfg(test)]
mod tests;

mod apply;

mod tokens;

fn take_following_value(
    argv: &mut std::iter::Peekable<impl Iterator<Item = (usize, OsString)>>,
    passthrough_from: Option<usize>,
    key: &str,
) -> Option<String> {
    let value = argv
        .peek()
        .filter(|&&(index, _)| !is_forwarded(passthrough_from, index))
        .and_then(|(_, token)| token.to_str())
        .filter(|token| claims_as_value(key, token))
        .map(str::to_owned)?;
    argv.next();
    Some(value)
}

impl ConfigOverrides {
    fn apply_token(
        &mut self,
        arg: OsString,
        claimed_by_command: &std::collections::HashSet<&str>,
        mut following: impl FnMut(&str) -> Option<String>,
    ) -> Option<OsString> {
        match classify(&arg, claimed_by_command) {
            ConfigToken::WellFormed { key, value } if matches!(key, "state-dir" | "store-dir") => {
                return Some(OsString::from(format!("--{key}={value}")));
            }
            ConfigToken::WellFormed { key, value } => self.set(key, value),
            ConfigToken::BooleanFollows(key) => {
                self.set(key, following(key).as_deref().unwrap_or("true"));
            }
            // A flag whose value is missing — because it ends argv, or
            // because the token after it is one the setting does not
            // take — goes back in place for clap to report; see
            // [`classify`].
            ConfigToken::ValueFollows(key) => match following(key) {
                Some(value) => self.set(key, &value),
                None => return Some(arg),
            },
            ConfigToken::Malformed => {}
            ConfigToken::NotOurs => return Some(arg),
        }
        None
    }
}

impl ConfigOverrides {}

mod assignment;
