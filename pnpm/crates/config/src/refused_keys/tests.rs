use super::{is_refused_by_a_project_manifest, where_refused_key_belongs};

/// The global config file takes these under a name of their own.
#[test]
fn a_key_the_global_config_file_takes_is_routed_there() {
    for (camel_key, kebab_key) in [
        ("stateDir", "state-dir"),
        ("globalDir", "global-dir"),
        ("globalBinDir", "global-bin-dir"),
        ("npmrcAuthFile", "npmrc-auth-file"),
    ] {
        assert_eq!(
            where_refused_key_belongs(camel_key),
            format!("Set it for the machine instead: pnpm config set --global {kebab_key}"),
        );
    }
}

/// These are derived from a key the global config file does take, and naming
/// their own spelling would send the user to a command that does nothing.
#[test]
fn a_derived_key_is_routed_to_the_key_it_derives_from() {
    for (camel_key, kebab_key) in [
        ("bin", "global-bin-dir"),
        ("globalPkgDir", "global-dir"),
        ("userconfig", "npmrc-auth-file"),
    ] {
        assert_eq!(
            where_refused_key_belongs(camel_key),
            format!("Set it for the machine instead: pnpm config set --global {kebab_key}"),
        );
    }
}

#[test]
fn dir_names_the_flag_that_sets_it() {
    assert_eq!(where_refused_key_belongs("dir"), "Pass --dir on the command line instead");
}

/// Naming where pnpm gets these would publish how it resolves them, and would
/// imply the key was a setting that only reached the wrong file.
#[test]
fn a_key_that_is_no_setting_is_not_offered_a_route() {
    for camel_key in [
        "configDir",
        "pnpmHomeDir",
        "rootProjectManifestDir",
        "workspaceDir",
        "authConfig",
        "userConfig",
        "configByUri",
        "packageManagerNetworkConfig",
        "packageManagerRegistries",
    ] {
        assert_eq!(where_refused_key_belongs(camel_key), "This is not a pnpm setting");
    }
}

/// The refusal reads both spellings, since a config file may carry either.
/// `pnpm login` turns `scope` into a `@scope:registry` route in the
/// machine-global `auth.ini`, which outranks `~/.npmrc` in every project on
/// the machine from then on, so the route offered is the global one.
/// See <https://github.com/pnpm/pnpm/issues/13557>
#[test]
fn the_login_scope_is_refused_and_routed_to_the_global_config() {
    assert!(is_refused_by_a_project_manifest("scope"));
    assert_eq!(
        where_refused_key_belongs("scope"),
        "Set it for the machine instead: pnpm config set --global scope",
    );
}

#[test]
fn refusal_is_spelling_insensitive() {
    assert!(is_refused_by_a_project_manifest("authConfig"));
    assert!(is_refused_by_a_project_manifest("auth-config"));
    assert!(is_refused_by_a_project_manifest("hooks"));
    assert!(!is_refused_by_a_project_manifest("storeDir"));
    assert!(!is_refused_by_a_project_manifest("node-linker"));
}
