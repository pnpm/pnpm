use super::{WorkspaceSettings, parse_json_or_string, parse_tri_array};
use crate::{
    NodeLinker, NodePackageMapType, SaveWorkspaceProtocol, ScriptsPrependNodePath, TrustPolicy,
    api::EnvVar,
};
use pretty_assertions::assert_eq;

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

/// An exported-but-empty `PNPM_CONFIG_STORE_DIR=` shouldn't clobber
/// the configured store path.
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

/// `savePrefix` is the exception to [`empty_env_var_is_treated_as_unset`]:
/// `""` is the value that pins an exact version, so
/// `PNPM_CONFIG_SAVE_PREFIX=` must reach the config as `Some("")` — the
/// same state `savePrefix: ""` in `pnpm-workspace.yaml` produces.
#[test]
fn empty_save_prefix_env_var_survives() {
    struct EnvEmptySavePrefix;
    impl EnvVar for EnvEmptySavePrefix {
        fn var(name: &str) -> Option<String> {
            (name == "PNPM_CONFIG_SAVE_PREFIX").then(String::new)
        }
    }
    let settings = WorkspaceSettings::from_pnpm_config_env::<EnvEmptySavePrefix>();
    assert_eq!(settings.save_prefix.as_deref(), Some(""));
}

/// `saveWorkspaceProtocol` is `boolean | "rolling"`, so its env var has
/// to accept both JSON booleans and the bare identifier.
#[test]
fn save_workspace_protocol_env_var_accepts_all_three_shapes() {
    for (value, expected) in [
        ("true", SaveWorkspaceProtocol::On),
        ("false", SaveWorkspaceProtocol::Off),
        ("rolling", SaveWorkspaceProtocol::Rolling),
    ] {
        assert_eq!(parse_json_or_string::<SaveWorkspaceProtocol>(value), Some(expected));
    }
    assert_eq!(parse_json_or_string::<SaveWorkspaceProtocol>("nonsense"), None);
}

/// The binding is wired to the `PNPM_CONFIG_*` name pnpm uses, not just
/// parseable in isolation.
#[test]
fn save_workspace_protocol_reads_from_the_environment() {
    struct EnvPinned;
    impl EnvVar for EnvPinned {
        fn var(name: &str) -> Option<String> {
            (name == "PNPM_CONFIG_SAVE_WORKSPACE_PROTOCOL").then(|| "true".to_owned())
        }
    }
    let settings = WorkspaceSettings::from_pnpm_config_env::<EnvPinned>();
    assert_eq!(settings.save_workspace_protocol, Some(SaveWorkspaceProtocol::On));
}

#[test]
fn enum_env_var_accepts_bare_identifier() {
    assert_eq!(parse_json_or_string::<NodeLinker>("hoisted"), Some(NodeLinker::Hoisted));
    assert_eq!(parse_json_or_string::<TrustPolicy>("no-downgrade"), Some(TrustPolicy::NoDowngrade));
    assert_eq!(
        parse_json_or_string::<NodePackageMapType>("loose"),
        Some(NodePackageMapType::Loose),
    );
}

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

#[test]
fn scope_parses_from_env() {
    struct EnvScope;
    impl EnvVar for EnvScope {
        fn var(name: &str) -> Option<String> {
            (name == "PNPM_CONFIG_SCOPE").then(|| "@my-org".to_owned())
        }
    }
    assert_eq!(
        WorkspaceSettings::from_pnpm_config_env::<EnvScope>().scope.as_deref(),
        Some("@my-org"),
    );
}

/// `scope`, like `savePrefix`, is an exception to
/// [`empty_env_var_is_treated_as_unset`]: `PNPM_CONFIG_SCOPE=` must reach the
/// config as `Some("")`, not `None`, so it clobbers a scope set by a
/// lower-priority layer and yields an unscoped `pnpm login` — matching the
/// TypeScript CLI, whose env pass keeps the empty value.
#[test]
fn empty_scope_env_var_survives_to_clobber_lower_layers() {
    struct EnvEmptyScope;
    impl EnvVar for EnvEmptyScope {
        fn var(name: &str) -> Option<String> {
            (name == "PNPM_CONFIG_SCOPE").then(String::new)
        }
    }
    let settings = WorkspaceSettings::from_pnpm_config_env::<EnvEmptyScope>();
    assert_eq!(settings.scope.as_deref(), Some(""));
}

#[test]
fn tri_array_env_var_parses_arrays_and_rejects_null() {
    assert_eq!(parse_tri_array(r#"["a","b"]"#), Some(Some(vec!["a".to_owned(), "b".to_owned()])));
    assert_eq!(parse_tri_array("null"), None);
    assert_eq!(parse_tri_array("not-json"), None);
}
