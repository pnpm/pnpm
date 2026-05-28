//! Read `PNPM_CONFIG_*` / `pnpm_config_*` environment variables into a
//! [`WorkspaceSettings`] overlay.
//!
//! Mirrors pnpm v11's
//! [`parseEnvVars`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/env.ts#L42)
//! loop in `config/reader/src/index.ts`, which reads `pnpm_config_<key>`
//! (or its `PNPM_CONFIG_<KEY>` uppercase form) for every key in the
//! schema and applies it to the config *after* `pnpm-workspace.yaml`.
//! That ordering means env vars override yaml — matching the order
//! upstream's loop runs in.
//!
//! Pacquet does NOT read `npm_config_*` / `NPM_CONFIG_*` env vars (with
//! the exception of `NPM_CONFIG_WORKSPACE_DIR`, which has its own narrow
//! handler in [`crate::Config::current`]). Pnpm v11 stopped honouring
//! those too; the only remaining `npm_config_*` lookup in pnpm is
//! `userconfig` as a low-priority auth-file fallback. See
//! [`config/reader/src/index.ts:719-722`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/index.ts#L719-L722).

use crate::{
    NodeLinker, PackageImportMethod, ScriptsPrependNodePath, TrustPolicy, WorkspaceSettings,
    api::EnvVar,
};
use serde::de::DeserializeOwned;

/// Read an env var by suffix, accepting both `PNPM_CONFIG_<UPPER>` and
/// `pnpm_config_<lower>`. Empty values are treated as unset, matching
/// upstream's
/// [`if (envValue == null) continue`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/env.ts#L46)
/// + the [`!== ''` filter in `readEnvVar`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/index.ts#L713).
fn read_env<Sys: EnvVar>(suffix: &str) -> Option<String> {
    let upper = format!("PNPM_CONFIG_{suffix}");
    let lower = format!("pnpm_config_{}", suffix.to_lowercase());
    Sys::var(&upper).or_else(|| Sys::var(&lower)).filter(|value| !value.is_empty())
}

/// Parse `value` as JSON. Returns `None` on parse failure so the
/// caller falls through to its default (skip the field).
fn parse_json<Target: DeserializeOwned>(value: &str) -> Option<Target> {
    serde_json::from_str(value).ok()
}

/// Parse `value` as JSON; if that fails, retry with `value` wrapped as
/// a JSON string. Used for enum fields whose serde representation is a
/// bare identifier (`hoisted`, `warn-only`, `no-downgrade`, …) — the
/// raw env var value isn't valid JSON on its own but becomes valid
/// once quoted.
fn parse_json_or_string<Target: DeserializeOwned>(value: &str) -> Option<Target> {
    parse_json(value).or_else(|| {
        let quoted = serde_json::to_string(value).ok()?;
        parse_json(&quoted)
    })
}

/// Parse a `hoist_pattern` / `public_hoist_pattern` env var into the
/// tri-state `Option<Option<Vec<String>>>` shape used by
/// [`WorkspaceSettings`].
///
/// Env vars cannot express the "explicit null disable" state that yaml
/// supports — pnpm's
/// [`parseValueByConstructor`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/env.ts#L111-L115)
/// for the `Array` schema runs `JSON.parse(envVar)` and then requires
/// `Array.isArray(value)`. `PNPM_CONFIG_HOIST_PATTERN=null` therefore
/// fails the `Array.isArray` check upstream and the value is silently
/// dropped. The tri-state's `Some(None)` branch stays reachable
/// through yaml only; from env we either return `None` (parse failed,
/// leave config default) or `Some(Some(vec))` (explicit list).
fn parse_tri_array(value: &str) -> Option<Option<Vec<String>>> {
    parse_json::<Vec<String>>(value).map(Some)
}

impl WorkspaceSettings {
    /// Build a [`WorkspaceSettings`] from `PNPM_CONFIG_*` env vars.
    ///
    /// Pnpm reads env vars for the full schema, not just config-file
    /// keys — `PNPM_CONFIG_HOIST=false`, `PNPM_CONFIG_NODE_LINKER=hoisted`
    /// etc. all work upstream, so they work here too (no
    /// [`Self::clear_workspace_only_fields`] call). Apply the returned
    /// settings via [`Self::apply_to`] *after* `pnpm-workspace.yaml` so
    /// env vars win over yaml, mirroring upstream's order at
    /// [`config/reader/src/index.ts:471-488`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/index.ts#L471-L488).
    pub fn from_pnpm_config_env<Sys: EnvVar>() -> Self {
        let mut settings = WorkspaceSettings::default();

        macro_rules! json_field {
            ($field:ident, $suffix:literal) => {
                if let Some(s) = read_env::<Sys>($suffix)
                    && let Some(v) = parse_json(&s)
                {
                    settings.$field = Some(v);
                }
            };
        }
        macro_rules! string_field {
            ($field:ident, $suffix:literal) => {
                if let Some(s) = read_env::<Sys>($suffix) {
                    settings.$field = Some(s);
                }
            };
        }
        macro_rules! enum_field {
            ($field:ident, $suffix:literal, $ty:ty) => {
                if let Some(s) = read_env::<Sys>($suffix)
                    && let Some(v) = parse_json_or_string::<$ty>(&s)
                {
                    settings.$field = Some(v);
                }
            };
        }
        macro_rules! tri_array_field {
            ($field:ident, $suffix:literal) => {
                if let Some(s) = read_env::<Sys>($suffix)
                    && let Some(v) = parse_tri_array(&s)
                {
                    settings.$field = Some(v);
                }
            };
        }

        json_field!(hoist, "HOIST");
        tri_array_field!(hoist_pattern, "HOIST_PATTERN");
        tri_array_field!(public_hoist_pattern, "PUBLIC_HOIST_PATTERN");
        json_field!(shamefully_hoist, "SHAMEFULLY_HOIST");
        string_field!(store_dir, "STORE_DIR");
        string_field!(modules_dir, "MODULES_DIR");
        enum_field!(node_linker, "NODE_LINKER", NodeLinker);
        json_field!(symlink, "SYMLINK");
        string_field!(virtual_store_dir, "VIRTUAL_STORE_DIR");
        json_field!(enable_global_virtual_store, "ENABLE_GLOBAL_VIRTUAL_STORE");
        string_field!(global_virtual_store_dir, "GLOBAL_VIRTUAL_STORE_DIR");
        enum_field!(package_import_method, "PACKAGE_IMPORT_METHOD", PackageImportMethod);
        json_field!(modules_cache_max_age, "MODULES_CACHE_MAX_AGE");
        json_field!(virtual_store_dir_max_length, "VIRTUAL_STORE_DIR_MAX_LENGTH");
        json_field!(lockfile, "LOCKFILE");
        json_field!(prefer_frozen_lockfile, "PREFER_FROZEN_LOCKFILE");
        json_field!(offline, "OFFLINE");
        json_field!(prefer_offline, "PREFER_OFFLINE");
        json_field!(lockfile_include_tarball_url, "LOCKFILE_INCLUDE_TARBALL_URL");
        string_field!(registry, "REGISTRY");
        json_field!(auto_install_peers, "AUTO_INSTALL_PEERS");
        json_field!(auto_install_peers_from_highest_match, "AUTO_INSTALL_PEERS_FROM_HIGHEST_MATCH");
        json_field!(hoist_workspace_packages, "HOIST_WORKSPACE_PACKAGES");
        json_field!(hoisting_limits, "HOISTING_LIMITS");
        json_field!(external_dependencies, "EXTERNAL_DEPENDENCIES");
        json_field!(dedupe_peer_dependents, "DEDUPE_PEER_DEPENDENTS");
        json_field!(dedupe_peers, "DEDUPE_PEERS");
        json_field!(prefer_workspace_packages, "PREFER_WORKSPACE_PACKAGES");
        json_field!(strict_peer_dependencies, "STRICT_PEER_DEPENDENCIES");
        json_field!(resolve_peers_from_workspace_root, "RESOLVE_PEERS_FROM_WORKSPACE_ROOT");
        json_field!(block_exotic_subdeps, "BLOCK_EXOTIC_SUBDEPS");
        json_field!(verify_store_integrity, "VERIFY_STORE_INTEGRITY");
        json_field!(side_effects_cache, "SIDE_EFFECTS_CACHE");
        json_field!(side_effects_cache_readonly, "SIDE_EFFECTS_CACHE_READONLY");
        json_field!(fetch_retries, "FETCH_RETRIES");
        json_field!(fetch_retry_factor, "FETCH_RETRY_FACTOR");
        json_field!(fetch_retry_mintimeout, "FETCH_RETRY_MINTIMEOUT");
        json_field!(fetch_retry_maxtimeout, "FETCH_RETRY_MAXTIMEOUT");
        json_field!(patched_dependencies, "PATCHED_DEPENDENCIES");
        json_field!(allow_builds, "ALLOW_BUILDS");
        json_field!(dangerously_allow_all_builds, "DANGEROUSLY_ALLOW_ALL_BUILDS");
        enum_field!(scripts_prepend_node_path, "SCRIPTS_PREPEND_NODE_PATH", ScriptsPrependNodePath);
        json_field!(unsafe_perm, "UNSAFE_PERM");
        json_field!(child_concurrency, "CHILD_CONCURRENCY");
        json_field!(workspace_concurrency, "WORKSPACE_CONCURRENCY");
        json_field!(git_shallow_hosts, "GIT_SHALLOW_HOSTS");
        json_field!(supported_architectures, "SUPPORTED_ARCHITECTURES");
        json_field!(ignored_optional_dependencies, "IGNORED_OPTIONAL_DEPENDENCIES");
        json_field!(overrides, "OVERRIDES");
        string_field!(cache_dir, "CACHE_DIR");
        json_field!(minimum_release_age, "MINIMUM_RELEASE_AGE");
        json_field!(minimum_release_age_exclude, "MINIMUM_RELEASE_AGE_EXCLUDE");
        json_field!(
            minimum_release_age_ignore_missing_time,
            "MINIMUM_RELEASE_AGE_IGNORE_MISSING_TIME"
        );
        json_field!(minimum_release_age_strict, "MINIMUM_RELEASE_AGE_STRICT");
        json_field!(trust_lockfile, "TRUST_LOCKFILE");
        enum_field!(trust_policy, "TRUST_POLICY", TrustPolicy);
        json_field!(trust_policy_exclude, "TRUST_POLICY_EXCLUDE");
        json_field!(trust_policy_ignore_after, "TRUST_POLICY_IGNORE_AFTER");

        settings
    }
}

#[cfg(test)]
mod tests {
    use super::{WorkspaceSettings, parse_json_or_string, parse_tri_array};
    use crate::{NodeLinker, ScriptsPrependNodePath, TrustPolicy, api::EnvVar};
    use pretty_assertions::assert_eq;

    /// Boolean env var values must be exact `true`/`false`. Anything
    /// else (capitalised, `yes`, `1`) silently falls through to the
    /// default — matching pnpm's
    /// [`parseValueByConstructor`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/env.ts#L117-L122)
    /// Boolean branch.
    #[test]
    fn bool_env_var_only_accepts_lowercase_true_false() {
        struct EnvBadBool;
        impl EnvVar for EnvBadBool {
            fn var(name: &str) -> Option<String> {
                (name == "PNPM_CONFIG_ENABLE_GLOBAL_VIRTUAL_STORE").then(|| "yes".to_owned())
            }
        }
        let settings = WorkspaceSettings::from_pnpm_config_env::<EnvBadBool>();
        assert_eq!(settings.enable_global_virtual_store, None);
    }

    /// Empty env var values are treated as unset, matching upstream's
    /// [`readEnvVar`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/index.ts#L711-L714)
    /// `value !== ''` filter — an exported-but-empty
    /// `PNPM_CONFIG_STORE_DIR=` shouldn't clobber the configured
    /// store path.
    #[test]
    fn empty_env_var_is_treated_as_unset() {
        struct EnvEmpty;
        impl EnvVar for EnvEmpty {
            fn var(name: &str) -> Option<String> {
                (name == "PNPM_CONFIG_STORE_DIR").then(String::new)
            }
        }
        let settings = WorkspaceSettings::from_pnpm_config_env::<EnvEmpty>();
        assert_eq!(settings.store_dir, None);
    }

    /// Enums parse from their kebab-case identifier without JSON
    /// quotes (`PNPM_CONFIG_NODE_LINKER=hoisted`) — `parse_json_or_string`
    /// retries the JSON parse with the value quoted when the bare
    /// parse fails, which is what makes the bare-identifier form work.
    #[test]
    fn enum_env_var_accepts_bare_identifier() {
        assert_eq!(parse_json_or_string::<NodeLinker>("hoisted"), Some(NodeLinker::Hoisted));
        assert_eq!(
            parse_json_or_string::<TrustPolicy>("no-downgrade"),
            Some(TrustPolicy::NoDowngrade),
        );
    }

    /// `ScriptsPrependNodePath` has a custom serde visitor that
    /// accepts `true` / `false` (booleans) or `"warn-only"` (string).
    /// The env-var path must hit all three — the JSON-first retry
    /// in `parse_json_or_string` ensures `true`/`false` reach the
    /// `visit_bool` arm and `warn-only` reaches the `visit_str` arm
    /// after the second pass quotes it.
    #[test]
    fn scripts_prepend_node_path_env_var_round_trips_all_three_shapes() {
        assert_eq!(
            parse_json_or_string::<ScriptsPrependNodePath>("true"),
            Some(ScriptsPrependNodePath::Always),
        );
        assert_eq!(
            parse_json_or_string::<ScriptsPrependNodePath>("false"),
            Some(ScriptsPrependNodePath::Never),
        );
        assert_eq!(
            parse_json_or_string::<ScriptsPrependNodePath>("warn-only"),
            Some(ScriptsPrependNodePath::WarnOnly),
        );
    }

    /// `PNPM_CONFIG_WORKSPACE_CONCURRENCY` parses as a JSON number
    /// into the signed `workspace_concurrency` slot — including the
    /// negative-offset form that upstream's `getWorkspaceConcurrency`
    /// reads as `parallelism - |value|`. The uppercase env name is
    /// honoured (the lowercase `pnpm_config_*` form is covered by the
    /// shared [`read_env`] helper).
    #[test]
    fn workspace_concurrency_env_var_parses_signed_number() {
        struct EnvPositive;
        impl EnvVar for EnvPositive {
            fn var(name: &str) -> Option<String> {
                (name == "PNPM_CONFIG_WORKSPACE_CONCURRENCY").then(|| "6".to_owned())
            }
        }
        assert_eq!(
            WorkspaceSettings::from_pnpm_config_env::<EnvPositive>().workspace_concurrency,
            Some(6),
        );

        struct EnvNegative;
        impl EnvVar for EnvNegative {
            fn var(name: &str) -> Option<String> {
                (name == "PNPM_CONFIG_WORKSPACE_CONCURRENCY").then(|| "-2".to_owned())
            }
        }
        assert_eq!(
            WorkspaceSettings::from_pnpm_config_env::<EnvNegative>().workspace_concurrency,
            Some(-2),
        );
    }

    /// Tri-state arrays (used by `hoist_pattern` /
    /// `public_hoist_pattern`): env vars carrying a JSON array
    /// populate the explicit-list slot. Pnpm's `Array` schema
    /// rejects `null` via its `Array.isArray` check at
    /// [`env.ts:111-115`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/env.ts#L111-L115),
    /// so `parse_tri_array("null")` returns `None` (= "leave the
    /// config default in place"), matching upstream. Plain non-JSON
    /// strings fall through the same way.
    #[test]
    fn tri_array_env_var_parses_arrays_and_rejects_null() {
        assert_eq!(
            parse_tri_array(r#"["a","b"]"#),
            Some(Some(vec!["a".to_owned(), "b".to_owned()])),
        );
        assert_eq!(parse_tri_array("null"), None);
        assert_eq!(parse_tri_array("not-json"), None);
    }
}
