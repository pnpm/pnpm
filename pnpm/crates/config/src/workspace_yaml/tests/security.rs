use super::{Config, Path, WorkspaceSettings, assert_eq};

/// `unsafePerm: false` from yaml propagates to `Config.unsafe_perm`
/// on POSIX. The starting `Config::new()` value depends on the runtime
/// uid (see [`default_unsafe_perm`]) — `true` for non-root, `false`
/// for root. Either way, `apply_to` with `Some(false)` ends in
/// `false`.
#[test]
fn parses_unsafe_perm_from_yaml_and_applies() {
    // POSIX-only: the Windows force-override below would mask this
    // test's behavior. See [`WorkspaceSettings::apply_to`].
    if cfg!(windows) {
        return;
    }
    let yaml = "unsafePerm: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    assert_eq!(settings.unsafe_perm, Some(false));

    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(!config.unsafe_perm, "yaml override wins on POSIX");
}

/// On Windows, `apply_to` ignores the yaml value and forces
/// `unsafe_perm = true` — running lifecycle scripts under a uid/gid
/// drop is POSIX-only.
#[cfg(windows)]
#[test]
fn unsafe_perm_force_true_on_windows() {
    let yaml = "unsafePerm: false\n";
    let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("C:/irrelevant"));
    assert!(config.unsafe_perm, "Windows forces unsafe_perm true regardless of yaml");
}

#[test]
fn keeps_bare_and_absolute_script_shell_values() {
    for script_shell in ["bash", "/usr/bin/bash"] {
        let mut settings: WorkspaceSettings =
            serde_saphyr::from_str(&format!("scriptShell: {script_shell}")).unwrap();
        settings.resolve_script_shell(Path::new("/workspace/root"));
        let mut config = Config::new();
        settings.apply_to(&mut config, Path::new("/workspace/root"));
        assert_eq!(config.script_shell.as_deref(), Some(script_shell));
    }
}

/// The tri-state distinguishes "absent" from "explicit null", matching
/// pnpm: an explicit `scriptShell: null` / `nodeOptions: null` clears a
/// value inherited from global `config.yaml`, while an absent key leaves
/// the inherited value untouched.
#[test]
fn script_shell_and_node_options_null_clears_inherited_value() {
    let absent: WorkspaceSettings = serde_saphyr::from_str("hoist: true").unwrap();
    assert_eq!(absent.script_shell, None);
    assert_eq!(absent.node_options, None);

    let mut config = Config::new();
    config.script_shell = Some("/inherited/sh".to_string());
    config.node_options = Some("--inherited".to_string());
    absent.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.script_shell.as_deref(), Some("/inherited/sh"), "absent must inherit");
    assert_eq!(config.node_options.as_deref(), Some("--inherited"), "absent must inherit");

    let cleared: WorkspaceSettings =
        serde_saphyr::from_str("scriptShell: null\nnodeOptions: null").unwrap();
    assert_eq!(cleared.script_shell, Some(None));
    assert_eq!(cleared.node_options, Some(None));

    let mut config = Config::new();
    config.script_shell = Some("/inherited/sh".to_string());
    config.node_options = Some("--inherited".to_string());
    cleared.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.script_shell, None, "explicit null must clear the inherited shell");
    assert_eq!(config.node_options, None, "explicit null must clear inherited NODE_OPTIONS");
}
