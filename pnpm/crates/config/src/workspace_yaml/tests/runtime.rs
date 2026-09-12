use super::{
    Config, GlobalShims, GlobalShimsSetting, Path, ScriptsPrependNodePath, ShimPolicy,
    WorkspaceSettings, assert_eq,
};

#[test]
fn global_shims_defaults_enable_the_managed_runtimes() {
    let shims = Config::default().global_shims;
    for name in ["node", "deno", "bun"] {
        assert!(shims.is_enabled(name), "{name} should be enabled by default");
    }
    assert!(!shims.is_enabled("typescript"));
    assert!(!shims.dispatches_nothing());
}

#[test]
fn global_shims_record_merges_over_the_defaults() {
    let settings: WorkspaceSettings =
        serde_saphyr::from_str("globalShims: {bun: false, typescript: true}\n").unwrap();
    assert!(matches!(settings.global_shims, Some(GlobalShimsSetting::Entries(_))));
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.global_shims.is_enabled("node"), "untouched defaults must survive");
    assert!(!config.global_shims.is_enabled("bun"), "one default can be switched off");
    assert!(config.global_shims.is_enabled("typescript"));
}

#[test]
fn global_shims_scalar_shorthands_reset_the_record() {
    let settings: WorkspaceSettings = serde_saphyr::from_str("globalShims: false\n").unwrap();
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.global_shims.dispatches_nothing());

    let settings: WorkspaceSettings = serde_saphyr::from_str("globalShims: true\n").unwrap();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.global_shims, GlobalShims::default());
}

#[test]
fn global_shims_named_policies_parse() {
    let settings: WorkspaceSettings =
        serde_saphyr::from_str("globalShims: {node: prompt, deno: always, bun: auto}\n").unwrap();
    let mut config = Config::default();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    let shims = config.global_shims;
    assert_eq!(shims.policy("node"), ShimPolicy::Prompt);
    assert_eq!(shims.policy("deno"), ShimPolicy::Always);
    assert_eq!(shims.policy("bun"), ShimPolicy::Auto, "explicit auto equals the true shorthand");
    assert_eq!(shims.policy("typescript"), ShimPolicy::Off);
    assert!(shims.is_enabled("node"), "prompt still counts as enabled");
}

#[test]
fn global_shims_later_layers_win_per_key() {
    let mut shims = GlobalShims::default();
    shims.apply(&serde_saphyr::from_str::<GlobalShimsSetting>("{node: false}").unwrap());
    shims
        .apply(&serde_saphyr::from_str::<GlobalShimsSetting>("{node: true, deno: false}").unwrap());
    assert!(shims.is_enabled("node"));
    assert!(!shims.is_enabled("deno"));
    assert!(shims.is_enabled("bun"));
}

/// `scriptsPrependNodePath` is a tri-state
/// (`boolean | 'warn-only'`): `true` → Always, `false` → Never,
/// `"warn-only"` → `WarnOnly`. Pacquet's default is Never.
#[test]
fn parses_scripts_prepend_node_path_true_from_yaml() {
    let yaml = "scriptsPrependNodePath: true\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.scripts_prepend_node_path, Some(ScriptsPrependNodePath::Always));

    let mut config = Config::new();
    assert_eq!(config.scripts_prepend_node_path, ScriptsPrependNodePath::Never, "default Never");
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.scripts_prepend_node_path, ScriptsPrependNodePath::Always);
}

#[test]
fn parses_scripts_prepend_node_path_false_from_yaml() {
    let yaml = "scriptsPrependNodePath: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.scripts_prepend_node_path, Some(ScriptsPrependNodePath::Never));
}

#[test]
fn parses_scripts_prepend_node_path_warn_only_from_yaml() {
    let yaml = "scriptsPrependNodePath: warn-only\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.scripts_prepend_node_path, Some(ScriptsPrependNodePath::WarnOnly));
}

#[test]
fn rejects_invalid_scripts_prepend_node_path() {
    let yaml = "scriptsPrependNodePath: nonsense\n";
    serde_saphyr::from_str::<WorkspaceSettings>(yaml).expect_err("must reject");
}

#[test]
fn parses_script_shell_and_node_options_from_yaml_and_applies() {
    let yaml = r"
scriptShell: /usr/bin/bash
nodeOptions: --max-old-space-size=4096
";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.script_shell, Some(Some("/usr/bin/bash".to_string())));
    assert_eq!(settings.node_options, Some(Some("--max-old-space-size=4096".to_string())));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.script_shell.as_deref(), Some("/usr/bin/bash"));
    assert_eq!(config.node_options.as_deref(), Some("--max-old-space-size=4096"));
}
