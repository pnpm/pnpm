use super::{
    ConfigOverrides, apply_registry_override, apply_state_dir_override, apply_store_dir_override,
};
use pnpm_config::{
    ColorMode, Config, EnvVar, GetCurrentDir, GetHomeDir, LinkProbe, LinkWorkspacePackages,
    NodeLinker, PackageImportMethod, PmOnFail, RemoteSideEffectsCacheSettings, RuntimeOnFail,
    SaveWorkspaceProtocol, TrustPolicy,
};
use pnpm_store_dir::STORE_VERSION;
use pretty_assertions::assert_eq;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

fn argv<Items: IntoIterator<Item = &'static str>>(items: Items) -> Vec<OsString> {
    items.into_iter().map(OsString::from).collect()
}

#[test]
fn extract_separates_config_tokens_from_argv() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "--config.registry=https://example.test/",
        "install",
        "--frozen-lockfile",
    ]));
    assert_eq!(remaining, argv(["pacquet", "install", "--frozen-lockfile"]));
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.registry, "https://example.test/");
    assert_eq!(config.package_manager_bootstrap.registry, "https://example.test/");
    assert_eq!(
        config.registries_by_scope.get("default").map(String::as_str),
        Some("https://example.test/"),
    );
    assert_eq!(
        config.package_manager_bootstrap.registries.get("default").map(String::as_str),
        Some("https://example.test/"),
    );
}

#[test]
fn extract_reads_the_login_scope() {
    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "--config.scope=@my-org", "login"]));
    assert_eq!(remaining, argv(["pacquet", "login"]));
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.scope.as_deref(), Some("@my-org"));
}

#[test]
fn extract_leaves_config_tokens_after_the_separator_for_the_child() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "run",
        "build",
        "--",
        "--pm-on-fail=ignore",
        "--config.registry=https://example.test/",
    ]));

    // Past `--` the tokens are the script's arguments, not pnpm's settings.
    assert_eq!(
        remaining,
        argv([
            "pacquet",
            "run",
            "build",
            "--",
            "--pm-on-fail=ignore",
            "--config.registry=https://example.test/",
        ]),
    );
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.pm_on_fail, None);
    assert_eq!(config.registry, Config::default().registry);
}

#[test]
fn registry_cli_override_normalizes_and_sets_every_registry_slot() {
    let mut config = Config::default();
    // No trailing slash on the input; it is normalized on the way in.
    apply_registry_override(&mut config, "https://cli.example");
    assert_eq!(config.registry, "https://cli.example/");
    assert_eq!(
        config.registries_by_scope.get("default").map(String::as_str),
        Some("https://cli.example/"),
    );
    assert_eq!(config.package_manager_bootstrap.registry, "https://cli.example/");
    assert_eq!(
        config.package_manager_bootstrap.registries.get("default").map(String::as_str),
        Some("https://cli.example/"),
    );
}

#[test]
fn extract_applies_scoped_registry_overrides() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "--config.@private:registry=https://private.example/npm",
        "install",
    ]));
    assert_eq!(remaining, argv(["pacquet", "install"]));
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(
        config.registries_by_scope.get("@private").map(String::as_str),
        Some("https://private.example/npm/"),
    );
    assert_eq!(
        config.package_manager_bootstrap.registries.get("@private").map(String::as_str),
        Some("https://private.example/npm/"),
    );
}

#[test]
fn scoped_registry_override_wins_over_existing_config() {
    let (overrides, _) =
        ConfigOverrides::extract(argv(["--config.@private:registry=https://cli.example/npm/"]));
    let mut config = Config::default();
    config
        .registries_by_scope
        .insert("@private".to_string(), "https://workspace.example/npm/".to_string());
    config
        .package_manager_bootstrap
        .registries
        .insert("@private".to_string(), "https://json-env.example/npm/".to_string());
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(
        config.registries_by_scope.get("@private").map(String::as_str),
        Some("https://cli.example/npm/"),
    );
    assert_eq!(
        config.package_manager_bootstrap.registries.get("@private").map(String::as_str),
        Some("https://cli.example/npm/"),
    );
}

#[test]
fn unknown_keys_are_dropped_silently() {
    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "--config.unknown-key=whatever", "install"]));
    assert_eq!(remaining, argv(["pacquet", "install"]));
    let default_registry = Config::default().registry;
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.registry, default_registry, "no known key set ⇒ registry untouched");
}

#[test]
fn extract_applies_the_minimum_release_age_overrides() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "--config.minimum-release-age=0",
        "--config.minimum-release-age-ignore-missing-time=false",
        "--config.minimum-release-age-strict=false",
        "add",
        "pnpm",
    ]));
    assert_eq!(remaining, argv(["pacquet", "add", "pnpm"]));
    let mut config = Config::default();
    assert_eq!(config.minimum_release_age, Some(1440));
    assert!(config.minimum_release_age_ignore_missing_time);
    assert_eq!(config.minimum_release_age_strict, None);
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.minimum_release_age, Some(0));
    assert!(!config.minimum_release_age_ignore_missing_time);
    assert_eq!(config.minimum_release_age_strict, Some(false));
    assert!(config.explicit_settings.contains_key("minimumReleaseAge"));
}

#[test]
fn max_sockets_overrides_win_over_the_config_layers_in_either_spelling() {
    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "--config.maxsockets=4", "install"]));
    assert_eq!(remaining, argv(["pacquet", "install"]));
    let mut config = Config { max_sockets: Some(2), ..Config::default() };
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.max_sockets, Some(4));

    let (overrides, _) = ConfigOverrides::extract(argv([
        "pacquet",
        "--config.maxsockets=4",
        "--config.max-sockets=9",
        "install",
    ]));
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.max_sockets, Some(9), "the canonical spelling wins over npm's");
}

#[test]
fn repeated_minimum_release_age_exclude_overrides_collect_into_a_list() {
    let (overrides, _) = ConfigOverrides::extract(argv([
        "--config.minimum-release-age-exclude=pnpm",
        "--config.minimum-release-age-exclude=@pnpm/exe",
    ]));
    let mut config = Config {
        minimum_release_age_exclude: Some(vec!["from-yaml".to_string()]),
        ..Config::default()
    };
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(
        config.minimum_release_age_exclude,
        Some(vec!["pnpm".to_string(), "@pnpm/exe".to_string()]),
    );
}

#[test]
fn extract_applies_ignore_scripts_override() {
    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "--config.ignore-scripts=true", "pack"]));
    assert_eq!(remaining, argv(["pacquet", "pack"]));
    let mut config = Config::default();
    assert!(!config.ignore_scripts);
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(config.ignore_scripts);
    assert_eq!(config.explicit_settings.get("ignoreScripts"), Some(&serde_json::Value::Bool(true)));

    let (overrides, _) =
        ConfigOverrides::extract(argv(["pacquet", "--config.ignore-scripts=false", "pack"]));
    let mut config = Config { ignore_scripts: true, ..Config::default() };
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(!config.ignore_scripts);
    assert_eq!(
        config.explicit_settings.get("ignoreScripts"),
        Some(&serde_json::Value::Bool(false)),
    );
}

#[test]
fn extract_applies_allow_unused_patches_override() {
    for (flag, expected) in [
        ("--allow-unused-patches", true),
        ("--allow-unused-patches=true", true),
        ("--allow-unused-patches=false", false),
        ("--allow-unused-patches=1", true),
        ("--allow-unused-patches=0", false),
        ("--config.allow-unused-patches=true", true),
        ("--config.allow-unused-patches=false", false),
        ("--config.allow-unused-patches=1", true),
        ("--config.allow-unused-patches=0", false),
        ("--no-allow-unused-patches", false),
    ] {
        let (overrides, remaining) = ConfigOverrides::extract(argv(["pacquet", flag, "deploy"]));
        assert_eq!(remaining, argv(["pacquet", "deploy"]));

        let mut config = Config { allow_unused_patches: !expected, ..Config::default() };
        overrides.apply(&mut config, Path::new("/workspace"));
        assert_eq!(config.allow_unused_patches, expected);
        assert_eq!(
            config.explicit_settings.get("allowUnusedPatches"),
            Some(&serde_json::Value::Bool(expected)),
        );
    }
}

#[test]
fn extract_leaves_invalid_allow_unused_patches_values_for_clap() {
    for flag in [
        "--allow-unused-patches=yes",
        "--allow-unused-patches=",
        "--config.allow-unused-patches=yes",
        "--config.allow-unused-patches=",
    ] {
        let (overrides, remaining) = ConfigOverrides::extract(argv(["pacquet", flag, "deploy"]));
        assert_eq!(remaining, argv(["pacquet", flag, "deploy"]));

        let mut config = Config::default();
        overrides.apply(&mut config, Path::new("/workspace"));
        assert!(!config.explicit_settings.contains_key("allowUnusedPatches"));
    }
}

#[test]
fn extract_applies_default_parity_overrides() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "--config.bail=false",
        "--config.ci=true",
        "--config.color=never",
        "--config.embed-readme=true",
        "--config.ignore-workspace-root-check=true",
        "--config.optional=false",
        "--config.package-lock=false",
        "--config.pending=true",
        "--config.recursive-install=false",
        "--config.reverse=true",
        "--config.shell-emulator=true",
        "--config.skip-manifest-obfuscation=true",
        "--config.sort=false",
        "--config.use-beta-cli=true",
        "install",
    ]));
    assert_eq!(remaining, argv(["pacquet", "install"]));

    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(!config.bail);
    assert!(config.ci);
    assert_eq!(config.color, ColorMode::Never);
    assert!(config.embed_readme);
    assert!(config.ignore_workspace_root_check);
    assert!(!config.optional);
    assert!(!config.package_lock);
    assert!(!config.lockfile);
    assert!(config.pending);
    assert!(!config.recursive_install);
    assert!(config.reverse);
    assert!(config.shell_emulator);
    assert!(config.skip_manifest_obfuscation);
    assert!(!config.sort);
    assert!(config.use_beta_cli);
}

#[test]
fn explicit_lockfile_override_wins_over_package_lock() {
    let (overrides, _) = ConfigOverrides::extract(argv([
        "pacquet",
        "--config.package-lock=false",
        "--config.lockfile=true",
        "install",
    ]));
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(!config.package_lock);
    assert!(config.lockfile);
}

#[test]
fn config_tokens_after_external_command_stay_in_argv() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "--config.registry=https://example.test/",
        "--dir",
        "project",
        "commitlint",
        "--config.foo=bar",
    ]));
    let expected = argv(["pacquet", "--dir", "project", "commitlint", "--config.foo=bar"]);
    assert_eq!(remaining, expected);
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.registry, "https://example.test/");
}

#[cfg(unix)]
#[test]
fn non_utf8_token_stops_config_token_extraction() {
    use std::os::unix::ffi::OsStringExt;

    let non_utf8 = OsString::from_vec(vec![0xff]);
    let (overrides, remaining) = ConfigOverrides::extract(vec![
        OsString::from("pacquet"),
        OsString::from("--config.registry=https://example.test/"),
        non_utf8.clone(),
        OsString::from("--config.foo=bar"),
    ]);
    let expected = vec![OsString::from("pacquet"), non_utf8, OsString::from("--config.foo=bar")];
    assert_eq!(remaining, expected);

    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.registry, "https://example.test/");
}

#[test]
fn malformed_tokens_are_dropped() {
    let (_, remaining) =
        ConfigOverrides::extract(argv(["--config.registry", "--config.=missing-key", "install"]));
    assert_eq!(remaining, argv(["install"]));
}

#[test]
fn last_value_wins_for_repeated_keys() {
    let (overrides, _) = ConfigOverrides::extract(argv([
        "--config.registry=https://first.test/",
        "--config.registry=https://second.test/",
    ]));
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.registry, "https://second.test/");
    assert_eq!(config.package_manager_bootstrap.registry, "https://second.test/");
}

#[test]
fn dotted_proxy_overrides_apply_to_network_config() {
    let (overrides, _) = ConfigOverrides::extract(argv([
        "pacquet",
        "install",
        "--config.https-proxy=http://proxy.example:8443",
        "--config.http-proxy=http://proxy.example:8080",
        "--config.no-proxy=localhost,127.0.0.1",
    ]));
    let mut config = Config::default();
    config.proxy.https_proxy = Some("http://yaml.example:9443".to_string());
    config.proxy.http_proxy = Some("http://yaml.example:9080".to_string());
    config.package_manager_bootstrap.proxy = config.proxy.clone();
    overrides.apply(&mut config, Path::new("/workspace"));

    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://proxy.example:8443"));
    assert_eq!(config.proxy.http_proxy.as_deref(), Some("http://proxy.example:8080"));
    assert_eq!(
        config.proxy.no_proxy,
        Some(pnpm_network::NoProxySetting::List(vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
        ])),
    );
    assert_eq!(config.package_manager_bootstrap.proxy, config.proxy);
}

#[test]
fn apply_is_a_noop_when_no_overrides_set() {
    let (overrides, _) = ConfigOverrides::extract(argv(["pacquet", "install"]));
    let default_registry = Config::default().registry;
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.registry, default_registry);
}

/// `linkWorkspacePackages` and `saveWorkspaceProtocol` are a boolean or a
/// keyword, so they take every boolean spelling plus the keyword. pnpm
/// types the first `[Boolean, 'deep']` and the second `Boolean`, so only
/// `deep` is spellable bare; `rolling` needs the `--config.` form.
#[test]
fn a_boolean_or_keyword_setting_takes_both_spellings() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "add",
        "foo",
        "--link-workspace-packages",
        "--config.save-workspace-protocol=rolling",
    ]));
    assert_eq!(remaining, argv(["pacquet", "add", "foo"]));
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.link_workspace_packages, LinkWorkspacePackages::DirectOnly);
    assert_eq!(config.save_workspace_protocol, SaveWorkspaceProtocol::Rolling);
    assert_eq!(
        config.explicit_settings.get("linkWorkspacePackages"),
        Some(&serde_json::Value::Bool(true)),
    );
    assert_eq!(
        config.explicit_settings.get("saveWorkspaceProtocol"),
        Some(&serde_json::Value::String("rolling".to_string())),
    );

    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "add",
        "foo",
        "--link-workspace-packages=deep",
        "--no-save-workspace-protocol",
    ]));
    assert_eq!(remaining, argv(["pacquet", "add", "foo"]));
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.link_workspace_packages, LinkWorkspacePackages::Deep);
    assert_eq!(config.save_workspace_protocol, SaveWorkspaceProtocol::Off);

    // The keyword is only taken in the `=` form; a following token is
    // claimed only when it spells a boolean.
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "add",
        "--link-workspace-packages",
        "false",
        "foo",
    ]));
    assert_eq!(remaining, argv(["pacquet", "add", "foo"]));
    let mut config =
        Config { link_workspace_packages: LinkWorkspacePackages::Deep, ..Config::default() };
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.link_workspace_packages, LinkWorkspacePackages::Off);

    // pnpm's `nopt` type for `saveWorkspaceProtocol` is `Boolean`, so the
    // bare spelling of the keyword is left for clap to report.
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "add",
        "foo",
        "--save-workspace-protocol=rolling",
    ]));
    assert_eq!(remaining, argv(["pacquet", "add", "foo", "--save-workspace-protocol=rolling"]));
    let mut config =
        Config { save_workspace_protocol: SaveWorkspaceProtocol::On, ..Config::default() };
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.save_workspace_protocol, SaveWorkspaceProtocol::On);
}

#[test]
fn install_keeps_the_trust_lockfile_pair_for_clap() {
    for flag in ["--trust-lockfile", "--no-trust-lockfile"] {
        let (overrides, remaining) = ConfigOverrides::extract(argv(["pacquet", "install", flag]));
        assert_eq!(remaining, argv(["pacquet", "install", flag]));

        let mut config = Config::default();
        overrides.apply(&mut config, Path::new("/workspace"));
        assert_eq!(config.trust_lockfile, Config::default().trust_lockfile, "{flag}");
    }
}

#[test]
fn a_value_taking_setting_reads_the_next_argv_token() {
    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "--package-import-method", "copy", "install"]));
    assert_eq!(remaining, argv(["pacquet", "install"]));

    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.package_import_method, PackageImportMethod::Copy);
}

#[test]
fn a_side_effects_cache_flag_replaces_the_object_form_and_keeps_its_remote_tier() {
    for (flag, declared_gates, expected) in
        [("--no-side-effects-cache", true, false), ("--side-effects-cache", false, true)]
    {
        let (overrides, remaining) = ConfigOverrides::extract(argv(["pacquet", "install", flag]));
        assert_eq!(remaining, argv(["pacquet", "install"]));

        let mut config = Config {
            side_effects_cache_read_setting: Some(declared_gates),
            side_effects_cache_write_setting: Some(declared_gates),
            remote_side_effects_cache: Some(RemoteSideEffectsCacheSettings {
                org: "acme".to_string(),
                ..RemoteSideEffectsCacheSettings::default()
            }),
            ..Config::default()
        };
        overrides.apply(&mut config, Path::new("/workspace"));

        assert_eq!(config.side_effects_cache_read(), expected);
        assert_eq!(config.side_effects_cache_write(), expected);
        assert_eq!(
            config.remote_side_effects_cache.map(|remote| remote.org),
            Some("acme".to_string()),
        );
    }
}

#[test]
fn a_repeated_list_setting_accumulates_its_values() {
    let (overrides, _) = ConfigOverrides::extract(argv([
        "pacquet",
        "install",
        "--hoist-pattern",
        "eslint",
        "--hoist-pattern=babel",
        "--public-hoist-pattern=types",
    ]));

    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.hoist_pattern, Some(vec!["eslint".to_string(), "babel".to_string()]));
    assert_eq!(config.public_hoist_pattern, Some(vec!["types".to_string()]));
}

/// `pnpm clean --lockfile` removes lockfiles; it does not turn the
/// `lockfile` setting on.
#[test]
fn a_command_option_wins_over_the_setting_of_the_same_name() {
    let (overrides, remaining) = ConfigOverrides::extract(argv(["pacquet", "clean", "--lockfile"]));
    assert_eq!(remaining, argv(["pacquet", "clean", "--lockfile"]));

    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.lockfile, Config::default().lockfile);
}

/// `pnpm install <pkg>` is pnpm's spelling of `pnpm add <pkg>`, so the
/// options it claims are `add`'s: `--offline` is `install`'s own option
/// but a setting to `add`.
#[test]
fn install_with_a_package_claims_the_options_of_add() {
    for command_line in [
        &["pacquet", "install", "valibot", "--offline", "--no-prefer-offline"][..],
        &["pacquet", "--offline", "--no-prefer-offline", "install", "valibot"],
        &["pacquet", "install", "--offline", "--no-prefer-offline", "--", "valibot"],
    ] {
        let (overrides, remaining) = ConfigOverrides::extract(argv(command_line.iter().copied()));
        let expected = command_line.iter().copied().filter(|token| !token.ends_with("offline"));
        assert_eq!(remaining, argv(expected), "{command_line:?}");

        let mut config = Config { prefer_offline: true, ..Config::default() };
        overrides.apply(&mut config, Path::new("/workspace"));
        assert!(config.offline, "{command_line:?}");
        assert!(!config.prefer_offline, "{command_line:?}");
        assert_eq!(
            config.explicit_settings.get("offline"),
            Some(&serde_json::Value::Bool(true)),
            "{command_line:?}",
        );
    }

    for command_line in [
        argv(["pacquet", "install", "--offline"]),
        argv(["pacquet", "install", "--offline", "--"]),
        argv(["pacquet", "install", "--reporter", "silent", "--offline"]),
    ] {
        let (overrides, remaining) = ConfigOverrides::extract(command_line.clone());
        assert_eq!(remaining, command_line);

        let mut config = Config::default();
        overrides.apply(&mut config, Path::new("/workspace"));
        assert!(!config.offline, "{command_line:?}");
    }
}

mod paths;

mod bare_flags;
