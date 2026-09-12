use super::{WorkspaceSettings, parse_json_or_string, parse_tri_array};
use crate::{
    ColorMode, Config, NodeLinker, NodePackageMapType, SaveWorkspaceProtocol,
    ScriptsPrependNodePath, TrustPolicy, VirtualStoreType, api::EnvVar,
};
use pretty_assertions::assert_eq;
use std::path::Path;

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

#[test]
fn materialization_settings_read_from_the_environment() {
    struct EnvMaterialization;
    impl EnvVar for EnvMaterialization {
        fn var(name: &str) -> Option<String> {
            match name {
                "PNPM_CONFIG_VIRTUAL_STORE_ONLY" => Some("true".to_owned()),
                "PNPM_CONFIG_ENABLE_MODULES_DIR" => Some("false".to_owned()),
                _ => None,
            }
        }
    }
    let settings = WorkspaceSettings::from_pnpm_config_env::<EnvMaterialization>();
    assert_eq!(settings.virtual_store_only, Some(true));
    assert_eq!(settings.enable_modules_dir, Some(false));
}

#[test]
fn allow_unused_patches_reads_from_the_environment() {
    struct EnvAllowUnusedPatches;
    impl EnvVar for EnvAllowUnusedPatches {
        fn var(name: &str) -> Option<String> {
            (name == "PNPM_CONFIG_ALLOW_UNUSED_PATCHES").then(|| "true".to_owned())
        }
    }
    let settings = WorkspaceSettings::from_pnpm_config_env::<EnvAllowUnusedPatches>();
    assert_eq!(settings.allow_unused_patches, Some(true));
}

#[test]
fn parity_settings_read_from_the_environment() {
    struct EnvParity;
    impl EnvVar for EnvParity {
        fn var(name: &str) -> Option<String> {
            match name {
                "PNPM_CONFIG_BAIL"
                | "PNPM_CONFIG_OPTIONAL"
                | "PNPM_CONFIG_PACKAGE_LOCK"
                | "PNPM_CONFIG_RECURSIVE_INSTALL"
                | "PNPM_CONFIG_SORT" => Some("false".to_owned()),
                "PNPM_CONFIG_EMBED_README"
                | "PNPM_CONFIG_IGNORE_WORKSPACE_ROOT_CHECK"
                | "PNPM_CONFIG_PENDING"
                | "PNPM_CONFIG_REVERSE"
                | "PNPM_CONFIG_SHELL_EMULATOR"
                | "PNPM_CONFIG_SKIP_MANIFEST_OBFUSCATION"
                | "PNPM_CONFIG_USE_BETA_CLI" => Some("true".to_owned()),
                "PNPM_CONFIG_COLOR" => Some("always".to_owned()),
                _ => None,
            }
        }
    }

    let settings = WorkspaceSettings::from_pnpm_config_env::<EnvParity>();
    assert_eq!(settings.bail, Some(false));
    assert_eq!(settings.color, Some(ColorMode::Always));
    assert_eq!(settings.embed_readme, Some(true));
    assert_eq!(settings.ignore_workspace_root_check, Some(true));
    assert_eq!(settings.optional, Some(false));
    assert_eq!(settings.package_lock, Some(false));
    assert_eq!(settings.pending, Some(true));
    assert_eq!(settings.recursive_install, Some(false));
    assert_eq!(settings.reverse, Some(true));
    assert_eq!(settings.shell_emulator, Some(true));
    assert_eq!(settings.skip_manifest_obfuscation, Some(true));
    assert_eq!(settings.sort, Some(false));
    assert_eq!(settings.use_beta_cli, Some(true));
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
                "PNPM_CONFIG_FETCH_WARN_TIMEOUT_MS" => Some("15000".to_owned()),
                "PNPM_CONFIG_FETCH_MIN_SPEED_KI_BPS" => Some("75".to_owned()),
                "PNPM_CONFIG_USER_AGENT" => Some("custom-ua/1.0".to_owned()),
                _ => None,
            }
        }
    }
    let settings = WorkspaceSettings::from_pnpm_config_env::<EnvNetwork>();
    assert_eq!(settings.network_concurrency, Some(12));
    assert_eq!(settings.fetch_timeout, Some(90_000));
    assert_eq!(settings.fetch_warn_timeout_ms, Some(15_000));
    assert_eq!(settings.fetch_min_speed_ki_bps, Some(75));
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

#[test]
fn virtual_store_type_env_var_parses_its_two_values() {
    macro_rules! env_with_virtual_store_type {
        ($name:ident, $value:expr) => {
            struct $name;
            impl EnvVar for $name {
                fn var(name: &str) -> Option<String> {
                    (name == "PNPM_CONFIG_VIRTUAL_STORE_TYPE").then(|| $value.to_owned())
                }
            }
        };
    }

    env_with_virtual_store_type!(EnvGlobal, "global");
    env_with_virtual_store_type!(EnvProject, "project");
    env_with_virtual_store_type!(EnvNonsense, "shared");

    assert_eq!(
        WorkspaceSettings::from_pnpm_config_env::<EnvGlobal>().virtual_store_type,
        Some(VirtualStoreType::Global),
    );
    assert_eq!(
        WorkspaceSettings::from_pnpm_config_env::<EnvProject>().virtual_store_type,
        Some(VirtualStoreType::Project),
    );
    assert_eq!(WorkspaceSettings::from_pnpm_config_env::<EnvNonsense>().virtual_store_type, None);
}

/// The environment can only spell the boolean, so it reaches the same
/// shorthand arm a yaml layer's boolean does: the object form's gates give way
/// to it and the remote tier it says nothing about survives.
#[test]
fn a_side_effects_cache_env_var_replaces_the_object_form() {
    struct EnvSideEffectsCacheOff;
    impl EnvVar for EnvSideEffectsCacheOff {
        fn var(name: &str) -> Option<String> {
            (name == "PNPM_CONFIG_SIDE_EFFECTS_CACHE").then(|| "false".to_owned())
        }
    }
    let declared: WorkspaceSettings = serde_saphyr::from_str(
        r"
sideEffectsCache:
  read: true
  write: true
  remote:
    org: acme
",
    )
    .unwrap();

    let mut config = Config::new();
    declared.apply_to(&mut config, Path::new("/workspace"));
    WorkspaceSettings::from_pnpm_config_env::<EnvSideEffectsCacheOff>()
        .apply_to(&mut config, Path::new("/workspace"));

    assert!(!config.side_effects_cache_read());
    assert!(!config.side_effects_cache_write());
    assert_eq!(config.remote_side_effects_cache.expect("shared cache config").org, "acme");
}
