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
    assert_eq!(parse_json_or_string::<TrustPolicy>("no-downgrade"), Some(TrustPolicy::NoDowngrade));
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

/// The network knobs read from `PNPM_CONFIG_*`:
/// `networkConcurrency` / `fetchTimeout` parse as JSON numbers,
/// `userAgent` as a raw string.
#[test]
fn network_settings_parse_from_env() {
    struct EnvNetwork;
    impl EnvVar for EnvNetwork {
        fn var(name: &str) -> Option<String> {
            match name {
                "PNPM_CONFIG_NETWORK_CONCURRENCY" => Some("12".to_owned()),
                "PNPM_CONFIG_FETCH_TIMEOUT" => Some("90000".to_owned()),
                "PNPM_CONFIG_USER_AGENT" => Some("custom-ua/1.0".to_owned()),
                _ => None,
            }
        }
    }
    let settings = WorkspaceSettings::from_pnpm_config_env::<EnvNetwork>();
    assert_eq!(settings.network_concurrency, Some(12));
    assert_eq!(settings.fetch_timeout, Some(90_000));
    assert_eq!(settings.user_agent.as_deref(), Some("custom-ua/1.0"));
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
    assert_eq!(parse_tri_array(r#"["a","b"]"#), Some(Some(vec!["a".to_owned(), "b".to_owned()])));
    assert_eq!(parse_tri_array("null"), None);
    assert_eq!(parse_tri_array("not-json"), None);
}
