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
fn extract_accepts_the_on_fail_settings_as_bare_flags() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "install",
        "--pm-on-fail=ignore",
        "--runtime-on-fail=warn",
    ]));
    assert_eq!(remaining, argv(["pacquet", "install"]));
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.pm_on_fail, Some(PmOnFail::Ignore));
    assert_eq!(config.runtime_on_fail, Some(RuntimeOnFail::Warn));
}

#[test]
fn extract_accepts_shamefully_hoist_cli_spellings() {
    for (flag, expected) in [
        ("--shamefully-hoist", true),
        ("--shamefully-hoist=true", true),
        ("--shamefully-hoist=false", false),
        ("--shamefully-hoist=1", true),
        ("--shamefully-hoist=0", false),
        ("--config.shamefully-hoist=true", true),
        ("--config.shamefully-hoist=false", false),
        ("--config.shamefully-hoist=1", true),
        ("--config.shamefully-hoist=0", false),
        ("--no-shamefully-hoist", false),
    ] {
        let (overrides, remaining) = ConfigOverrides::extract(argv(["pacquet", flag, "--version"]));
        assert_eq!(remaining, argv(["pacquet", "--version"]));

        let mut config = Config::default();
        overrides.apply(&mut config, Path::new("/workspace"));
        assert_eq!(config.shamefully_hoist, expected);
        let expected_public_hoist_pattern = expected.then(|| vec!["*".to_string()]);
        assert_eq!(config.public_hoist_pattern, expected_public_hoist_pattern);
        assert_eq!(
            config.explicit_settings.get("shamefullyHoist"),
            Some(&serde_json::Value::Bool(expected)),
        );
    }
}

#[test]
fn shamefully_hoist_override_preserves_virtual_store_only_precedence() {
    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "--shamefully-hoist=true", "install"]));
    assert_eq!(remaining, argv(["pacquet", "install"]));

    let mut config = Config {
        virtual_store_only: true,
        hoist_pattern: Some(vec!["*".to_string()]),
        ..Config::default()
    };
    overrides.apply(&mut config, Path::new("/workspace"));

    assert_eq!(config.hoist_pattern, Some(Vec::new()));
    assert_eq!(config.public_hoist_pattern, Some(Vec::new()));
}

#[test]
fn extract_leaves_invalid_shamefully_hoist_values_for_clap() {
    for flag in [
        "--shamefully-hoist=yes",
        "--shamefully-hoist=",
        "--config.shamefully-hoist=yes",
        "--config.shamefully-hoist=",
    ] {
        let (overrides, remaining) = ConfigOverrides::extract(argv(["pacquet", flag, "--version"]));
        assert_eq!(remaining, argv(["pacquet", flag, "--version"]));

        let mut config = Config::default();
        overrides.apply(&mut config, Path::new("/workspace"));
        assert!(!config.explicit_settings.contains_key("shamefullyHoist"));
    }
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
fn extract_leaves_other_bare_flags_for_clap() {
    let (_, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "install", "--node-linker=hoisted"]));
    assert_eq!(remaining, argv(["pacquet", "install", "--node-linker=hoisted"]));
}

#[test]
fn extract_rewrites_the_dotted_state_dir_for_clap() {
    let (_, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "--config.state-dir=/custom/state", "install"]));
    assert_eq!(remaining, argv(["pacquet", "--state-dir=/custom/state", "install"]));
}

#[test]
fn state_dir_cli_override_is_recorded() {
    let anchor = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    apply_state_dir_override::<pnpm_config::Host>(
        &mut config,
        PathBuf::from("custom-state").as_path(),
        anchor.path(),
    );
    assert_eq!(config.state_dir, anchor.path().join("custom-state"));
    assert_eq!(
        config.explicit_settings.get("stateDir"),
        Some(&serde_json::Value::String("custom-state".to_string())),
    );
}

#[test]
fn absolute_state_dir_cli_override_is_preserved() {
    let root = tempfile::tempdir().unwrap();
    let state_dir = root.path().join("absolute-state");
    let mut config = Config::default();
    apply_state_dir_override::<pnpm_config::Host>(
        &mut config,
        &state_dir,
        &root.path().join("unrelated-anchor"),
    );
    assert_eq!(config.state_dir, state_dir);
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
fn extract_applies_inject_workspace_packages_and_node_linker_overrides() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "--config.inject-workspace-packages=true",
        "--config.node-linker=hoisted",
        "deploy",
        "target",
    ]));
    assert_eq!(remaining, argv(["pacquet", "deploy", "target"]));
    let mut config = Config::default();
    assert!(!config.inject_workspace_packages);
    assert_eq!(config.node_linker, NodeLinker::Isolated);
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(config.inject_workspace_packages);
    assert_eq!(config.node_linker, NodeLinker::Hoisted);
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
fn node_linker_override_rederives_prefer_symlinked_executables() {
    // Overriding away from hoisted drops the derived `true`.
    let (overrides, _) =
        ConfigOverrides::extract(argv(["pacquet", "--config.node-linker=isolated", "install"]));
    let mut config = Config { node_linker: NodeLinker::Hoisted, ..Config::default() };
    config.apply_prefer_symlinked_executables_derivation();
    assert_eq!(config.prefer_symlinked_executables, Some(true));
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.node_linker, NodeLinker::Isolated);
    assert_eq!(config.prefer_symlinked_executables, None);

    // Overriding to hoisted derives `true`, like pnpm's config reader
    // seeing the CLI-selected linker.
    let (overrides, _) =
        ConfigOverrides::extract(argv(["pacquet", "--config.node-linker=hoisted", "install"]));
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.prefer_symlinked_executables, Some(true));

    // An explicit `false` — recorded in `explicit_settings` by the
    // config layer that set it — outranks the hoisted default.
    let (overrides, _) =
        ConfigOverrides::extract(argv(["pacquet", "--config.node-linker=hoisted", "install"]));
    let mut config = Config { prefer_symlinked_executables: Some(false), ..Config::default() };
    config
        .explicit_settings
        .insert("preferSymlinkedExecutables".to_string(), serde_json::Value::Bool(false));
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.node_linker, NodeLinker::Hoisted);
    assert_eq!(config.prefer_symlinked_executables, Some(false));
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

#[test]
fn dotted_store_dir_is_rewritten_for_the_global_parser() {
    let (_, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "install", "--config.store-dir=dotted-store"]));
    assert_eq!(remaining, argv(["pacquet", "install", "--store-dir=dotted-store"]));
}

#[test]
fn store_dir_override_resolves_from_workspace_root() {
    struct FakeHome;

    impl GetHomeDir for FakeHome {
        fn home_dir() -> Option<PathBuf> {
            unreachable!("relative store directory does not consult the home directory")
        }
    }

    impl EnvVar for FakeHome {
        fn var(_: &str) -> Option<String> {
            unreachable!("relative store directory does not consult environment variables")
        }
    }

    impl GetCurrentDir for FakeHome {
        fn current_dir() -> std::io::Result<PathBuf> {
            unreachable!("relative store directory does not consult the current directory")
        }
    }

    impl LinkProbe for FakeHome {
        fn can_link_between_dirs(_: &std::path::Path, _: &std::path::Path) -> bool {
            unreachable!("relative store directory does not probe filesystem linkability")
        }
    }

    let temp_dir = std::env::temp_dir();
    let workspace_dir = temp_dir.join("pacquet-store-dir-workspace");
    let package_dir = workspace_dir.join("packages/app");
    let mut config = Config { workspace_dir: Some(workspace_dir.clone()), ..Config::default() };

    apply_store_dir_override::<FakeHome>(
        &mut config,
        std::path::Path::new("relative-store"),
        &package_dir,
    )
    .expect("resolve relative store directory");

    assert_eq!(config.store_dir.root(), workspace_dir.join("relative-store").join(STORE_VERSION));
}

#[test]
fn store_dir_override_expands_quoted_home_path() {
    struct FakeHome;

    impl GetHomeDir for FakeHome {
        fn home_dir() -> Option<PathBuf> {
            Some(std::env::temp_dir().join("pacquet-store-dir-home"))
        }
    }

    impl EnvVar for FakeHome {
        fn var(_: &str) -> Option<String> {
            unreachable!("home-relative store directory does not consult environment variables")
        }
    }

    impl GetCurrentDir for FakeHome {
        fn current_dir() -> std::io::Result<PathBuf> {
            unreachable!("home-relative store directory does not consult the current directory")
        }
    }

    impl LinkProbe for FakeHome {
        fn can_link_between_dirs(_: &std::path::Path, _: &std::path::Path) -> bool {
            unreachable!("home-relative store directory does not probe filesystem linkability")
        }
    }

    let mut config = Config::default();
    apply_store_dir_override::<FakeHome>(
        &mut config,
        std::path::Path::new("~/quoted-store"),
        std::path::Path::new("ignored-package-dir"),
    )
    .expect("expand home-relative store directory");

    assert_eq!(
        config.store_dir.root(),
        std::env::temp_dir().join("pacquet-store-dir-home/quoted-store").join(STORE_VERSION),
    );
    assert_eq!(
        config.explicit_settings.get("storeDir"),
        Some(&serde_json::Value::String("~/quoted-store".to_string())),
    );
}

#[test]
fn empty_store_dir_override_uses_the_injected_default_provider() {
    struct FakeDefault;

    impl EnvVar for FakeDefault {
        fn var(name: &str) -> Option<String> {
            (name == "PNPM_HOME").then(|| "/fake/pnpm-home".to_string())
        }
    }

    impl GetCurrentDir for FakeDefault {
        fn current_dir() -> std::io::Result<PathBuf> {
            unreachable!("PNPM_HOME determines the default before the current directory is needed")
        }
    }

    impl GetHomeDir for FakeDefault {
        fn home_dir() -> Option<PathBuf> {
            Some(PathBuf::from("/fake/home"))
        }
    }

    impl LinkProbe for FakeDefault {
        fn can_link_between_dirs(_: &std::path::Path, _: &std::path::Path) -> bool {
            true
        }
    }

    let workspace_dir = std::env::temp_dir();
    let mut config = Config { workspace_dir: Some(workspace_dir.clone()), ..Config::default() };

    apply_store_dir_override::<FakeDefault>(&mut config, std::path::Path::new(""), &workspace_dir)
        .expect("restore the default store directory");

    assert_eq!(
        config.store_dir.root(),
        std::path::Path::new("/fake/pnpm-home/store").join(STORE_VERSION),
    );
    assert_eq!(
        config.explicit_settings.get("storeDir"),
        Some(&serde_json::Value::String(String::new())),
    );
}

#[test]
fn extract_accepts_the_install_settings_as_bare_flags() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "install",
        "--package-import-method=hardlink",
        "--child-concurrency=3",
        "--strict-peer-dependencies",
        "--side-effects-cache",
        "--side-effects-cache-readonly",
        "--optimistic-repeat-install",
        "--trust-policy=no-downgrade",
        "--trust-policy-exclude=lodash",
        "--trust-policy-ignore-after=5",
        "--no-lockfile",
    ]));
    assert_eq!(remaining, argv(["pacquet", "install"]));

    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.package_import_method, PackageImportMethod::Hardlink);
    assert_eq!(config.child_concurrency, 3);
    assert!(config.strict_peer_dependencies);
    assert!(config.side_effects_cache);
    assert!(config.side_effects_cache_readonly);
    assert!(config.optimistic_repeat_install);
    assert_eq!(config.trust_policy, TrustPolicy::NoDowngrade);
    assert_eq!(config.trust_policy_exclude, Some(vec!["lodash".to_string()]));
    assert_eq!(config.trust_policy_ignore_after, Some(5));
    assert!(!config.lockfile);
}

/// `install` declares `--trust-lockfile` itself; every other command
/// takes the spelling from the table, so it lands on [`Config`] before
/// the command reads `config.trust_lockfile`.
#[test]
fn trust_lockfile_is_a_bare_flag_where_no_command_declares_it() {
    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "remove", "foo", "--trust-lockfile"]));
    assert_eq!(remaining, argv(["pacquet", "remove", "foo"]));
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(config.trust_lockfile);
    assert_eq!(config.explicit_settings.get("trustLockfile"), Some(&serde_json::Value::Bool(true)));

    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "remove", "foo", "--no-trust-lockfile"]));
    assert_eq!(remaining, argv(["pacquet", "remove", "foo"]));
    let mut config = Config { trust_lockfile: true, ..Config::default() };
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(!config.trust_lockfile);

    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "--config.trust-lockfile=true", "update"]));
    assert_eq!(remaining, argv(["pacquet", "update"]));
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(config.trust_lockfile);
}

/// Vercel runs every pnpm install as `pnpm install --unsafe-perm`
/// ([pnpm/pnpm#14346](https://github.com/pnpm/pnpm/issues/14346)).
#[test]
fn unsafe_perm_is_a_bare_flag_on_every_command() {
    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "install", "--unsafe-perm"]));
    assert_eq!(remaining, argv(["pacquet", "install"]));
    let mut config = Config { unsafe_perm: false, ..Config::default() };
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(config.unsafe_perm);
    assert_eq!(config.explicit_settings.get("unsafePerm"), Some(&serde_json::Value::Bool(true)));

    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "rebuild", "--no-unsafe-perm"]));
    assert_eq!(remaining, argv(["pacquet", "rebuild"]));
    let mut config = Config { unsafe_perm: true, ..Config::default() };
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(!config.unsafe_perm);
    assert_eq!(config.explicit_settings.get("unsafePerm"), Some(&serde_json::Value::Bool(false)));

    for (flag, expected) in [
        ("--unsafe-perm", true),
        ("--unsafe-perm=true", true),
        ("--no-unsafe-perm", false),
        ("--unsafe-perm=false", false),
    ] {
        let (overrides, remaining) =
            ConfigOverrides::extract(argv(["pacquet", "remove", "foo", flag]));
        assert_eq!(remaining, argv(["pacquet", "remove", "foo"]), "{flag}");
        let mut config = Config { unsafe_perm: !expected, ..Config::default() };
        overrides.apply(&mut config, Path::new("/workspace"));
        assert_eq!(config.unsafe_perm, expected, "{flag}");
    }
}

/// The boolean settings pnpm's `nopt` types make spellable on every
/// command that lists them, which clap rejected as unexpected arguments.
#[test]
fn the_boolean_settings_are_bare_flags_where_no_command_declares_them() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "add",
        "foo",
        "--dangerously-allow-all-builds",
        "--engine-strict",
        "--frozen-store",
        "--lockfile-include-tarball-url",
        "--merge-git-branch-lockfiles",
        "--node-experimental-package-map",
        "--offline",
        "--prefer-frozen-lockfile",
        "--prefer-offline",
        "--no-shared-workspace-lockfile",
        "--no-verify-store-integrity",
        "--force-legacy-deploy",
    ]));
    assert_eq!(remaining, argv(["pacquet", "add", "foo"]));

    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(config.dangerously_allow_all_builds);
    assert!(config.engine_strict);
    assert!(config.frozen_store);
    assert!(config.lockfile_include_tarball_url);
    assert!(config.merge_git_branch_lockfiles);
    assert!(config.node_experimental_package_map);
    assert!(config.offline);
    assert!(config.prefer_frozen_lockfile);
    assert!(config.prefer_offline);
    assert!(!config.shared_workspace_lockfile);
    assert!(!config.verify_store_integrity);
    assert!(config.force_legacy_deploy);
    assert_eq!(config.explicit_settings.get("offline"), Some(&serde_json::Value::Bool(true)));
    assert_eq!(
        config.explicit_settings.get("sharedWorkspaceLockfile"),
        Some(&serde_json::Value::Bool(false)),
    );

    // `audit` declares neither, unlike `install` and `add`.
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "audit",
        "--ignore-scripts",
        "--ignore-pnpmfile",
    ]));
    assert_eq!(remaining, argv(["pacquet", "audit"]));
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(config.ignore_scripts);
    assert!(config.ignore_pnpmfile);
}

/// `virtualStoreOnly` empties the hoist patterns the way the yaml layer
/// does, so a later install does not read a pattern this one never applied.
#[test]
fn virtual_store_only_flag_empties_the_hoist_patterns() {
    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "install", "--virtual-store-only"]));
    assert_eq!(remaining, argv(["pacquet", "install"]));
    let mut config = Config {
        hoist_pattern: Some(vec!["*".to_string()]),
        public_hoist_pattern: Some(vec!["*eslint*".to_string()]),
        ..Config::default()
    };
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(config.virtual_store_only);
    assert_eq!(config.hoist_pattern, Some(Vec::new()));
    assert_eq!(config.public_hoist_pattern, Some(Vec::new()));
}

/// A lower layer's `virtualStoreOnly: true` empties the patterns when the
/// config is built; `--no-virtual-store-only` outranks it and gets them
/// back exactly, an explicitly disabled pattern included.
#[test]
fn no_virtual_store_only_restores_the_hoist_patterns() {
    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "install", "--no-virtual-store-only"]));
    assert_eq!(remaining, argv(["pacquet", "install"]));

    let mut config = Config {
        virtual_store_only: true,
        hoist_pattern: Some(vec!["*eslint*".to_string()]),
        public_hoist_pattern: None,
        ..Config::default()
    };
    config.apply_virtual_store_only_derivation();
    assert_eq!(config.hoist_pattern, Some(Vec::new()));
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(!config.virtual_store_only);
    assert_eq!(config.hoist_pattern, Some(vec!["*eslint*".to_string()]));
    assert_eq!(config.public_hoist_pattern, None);
    assert_eq!(config.hoist_patterns_before_virtual_store_only, None);

    // A pattern given on the same command line is what comes back.
    let (overrides, _) = ConfigOverrides::extract(argv([
        "pacquet",
        "install",
        "--hoist-pattern=foo",
        "--no-virtual-store-only",
    ]));
    let mut config = Config { virtual_store_only: true, ..Config::default() };
    config.apply_virtual_store_only_derivation();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.hoist_pattern, Some(vec!["foo".to_string()]));
    assert_eq!(config.public_hoist_pattern, Config::default().public_hoist_pattern);

    // Turning the mode on from the command line keeps the patterns
    // empty whatever else the command line says about them.
    let (overrides, _) = ConfigOverrides::extract(argv([
        "pacquet",
        "install",
        "--virtual-store-only",
        "--hoist-pattern=foo",
    ]));
    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.hoist_pattern, Some(Vec::new()));
    assert_eq!(config.public_hoist_pattern, Some(Vec::new()));
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
fn a_boolean_setting_claims_the_next_token_only_when_it_spells_a_boolean() {
    for (tokens, expected) in [
        (vec!["--side-effects-cache", "install"], true),
        (vec!["--side-effects-cache", "false", "install"], false),
        (vec!["--side-effects-cache", "true", "install"], true),
    ] {
        let (overrides, remaining) =
            ConfigOverrides::extract(argv(["pacquet"]).into_iter().chain(argv(tokens)));
        assert_eq!(remaining, argv(["pacquet", "install"]));

        let mut config = Config::default();
        overrides.apply(&mut config, Path::new("/workspace"));
        assert_eq!(config.side_effects_cache, expected);
    }
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

#[test]
fn no_hoist_clears_the_private_hoist_pattern() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "install",
        "--no-hoist",
        "--hoist-pattern=eslint",
    ]));
    assert_eq!(remaining, argv(["pacquet", "install"]));

    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(!config.hoist);
    assert_eq!(config.hoist_pattern, None);
}

#[test]
fn shamefully_hoist_wins_over_a_public_hoist_pattern_on_the_same_command_line() {
    let (overrides, _) = ConfigOverrides::extract(argv([
        "pacquet",
        "install",
        "--public-hoist-pattern=types",
        "--shamefully-hoist",
    ]));

    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.public_hoist_pattern, Some(vec!["*".to_string()]));
}

#[test]
fn the_modules_and_virtual_store_dirs_are_anchored_at_the_workspace_root() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "install",
        "--modules-dir=custom_modules",
        "--virtual-store-dir=custom_store",
    ]));
    assert_eq!(remaining, argv(["pacquet", "install"]));

    let workspace_dir = PathBuf::from("/workspace");
    let mut config = Config { workspace_dir: Some(workspace_dir.clone()), ..Config::default() };
    overrides.apply(&mut config, Path::new("/workspace/pkg"));
    assert_eq!(config.modules_dir, workspace_dir.join("custom_modules"));
    assert_eq!(config.virtual_store_dir, workspace_dir.join("custom_store"));
}

#[test]
fn the_modules_dir_alone_re_anchors_the_default_virtual_store() {
    let (overrides, _) =
        ConfigOverrides::extract(argv(["pacquet", "install", "--modules-dir=custom_modules"]));

    let workspace_dir = PathBuf::from("/workspace");
    let mut config = Config { workspace_dir: Some(workspace_dir.clone()), ..Config::default() };
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.virtual_store_dir, workspace_dir.join("custom_modules/.pnpm"));
}

#[test]
fn the_global_dir_override_re_derives_the_global_package_dir() {
    let (overrides, remaining) =
        ConfigOverrides::extract(argv(["pacquet", "add", "-g", "--global-dir", "/custom/global"]));
    assert_eq!(remaining, argv(["pacquet", "add", "-g"]));

    let mut config = Config::default();
    overrides.apply(&mut config, Path::new("/workspace"));
    assert_eq!(config.global_dir.as_deref(), Some(Path::new("/custom/global")));
    assert_eq!(
        config.global_pkg_dir,
        Some(Path::new("/custom/global").join(pnpm_config::GLOBAL_LAYOUT_VERSION)),
    );
}

#[test]
fn a_setting_flag_does_not_swallow_the_command_it_precedes() {
    let (_, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "--modules-dir",
        "custom_modules",
        "run",
        "build",
        "--hoist-pattern=eslint",
    ]));

    // `--hoist-pattern` is past the script name, so it is the script's.
    assert_eq!(remaining, argv(["pacquet", "run", "build", "--hoist-pattern=eslint"]));
}

/// A setting is stripped from argv before clap runs, so one spelled like
/// a *global* option would claim that option on every command line — a
/// command's own option is left for clap instead, and can collide.
#[test]
fn no_bare_setting_flag_shadows_a_global_option() {
    let grammar = crate::cli_args::grammar();
    let declared: Vec<&str> = grammar
        .get_arguments()
        .flat_map(|arg| {
            arg.get_long().into_iter().chain(arg.get_all_aliases().into_iter().flatten())
        })
        .collect();
    let shadowed: Vec<&str> = super::BARE_SETTING_FLAGS
        .iter()
        .map(|&(setting, _)| setting)
        .filter(|setting| declared.contains(setting))
        .collect();
    assert_eq!(shadowed, Vec::<&str>::new());
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

/// A value the setting does not take must reach clap, which reports it:
/// dropping `--trust-policy=typo` would run the install under the default
/// `off` policy the user meant to replace.
#[test]
fn extract_leaves_invalid_setting_values_for_clap() {
    for tokens in [
        ["--trust-policy=typo"].as_slice(),
        &["--trust-policy", "typo"],
        &["--config.trust-policy=typo"],
        &["--package-import-method=symlink"],
        &["--child-concurrency=lots"],
        &["--child-concurrency", "lots"],
        &["--trust-policy-ignore-after=soon"],
        &["--link-workspace-packages=shallow"],
        &["--save-workspace-protocol=sometimes"],
        &["--offline=maybe"],
        &["--unsafe-perm=maybe"],
    ] {
        let command_line = ["pacquet", "install"]
            .into_iter()
            .chain(tokens.iter().copied())
            .map(OsString::from)
            .collect::<Vec<_>>();
        let (overrides, remaining) = ConfigOverrides::extract(command_line.clone());
        assert_eq!(remaining, command_line, "{tokens:?}");

        let mut config = Config::default();
        overrides.apply(&mut config, Path::new("/workspace"));
        let defaults = Config::default();
        assert_eq!(config.trust_policy, defaults.trust_policy, "{tokens:?}");
        assert_eq!(config.package_import_method, defaults.package_import_method, "{tokens:?}");
        assert_eq!(config.child_concurrency, defaults.child_concurrency, "{tokens:?}");
        assert_eq!(
            config.trust_policy_ignore_after, defaults.trust_policy_ignore_after,
            "{tokens:?}",
        );
    }
}

/// The boundary scan and the extraction have to agree on how much a bare
/// boolean setting claims, or the settings after an explicit `true` /
/// `false` are mistaken for a script's arguments and forwarded to clap.
#[test]
fn a_boolean_settings_explicit_value_does_not_move_the_command_boundary() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "--strict-peer-dependencies",
        "false",
        "--config.registry=https://example.test/",
        "install",
    ]));
    assert_eq!(remaining, argv(["pacquet", "install"]));

    let mut config = Config { strict_peer_dependencies: true, ..Config::default() };
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(!config.strict_peer_dependencies);
    assert_eq!(config.registry, "https://example.test/");
}

/// `lockfile` is both a setting and `clean`'s own option, so the boundary
/// scan and the extraction have to agree on how much `--lockfile true`
/// claims even when the command is not `clean`.
#[test]
fn a_boolean_settings_value_is_claimed_even_when_a_command_declares_the_name() {
    let (overrides, remaining) = ConfigOverrides::extract(argv([
        "pacquet",
        "--lockfile",
        "true",
        "--config.registry=https://example.test/",
        "install",
    ]));
    assert_eq!(remaining, argv(["pacquet", "install"]));

    let mut config = Config { lockfile: false, ..Config::default() };
    overrides.apply(&mut config, Path::new("/workspace"));
    assert!(config.lockfile);
    assert_eq!(config.registry, "https://example.test/");
}

/// A `--` or another flag is never a free-form setting's value: claiming
/// one would drop the separator and point `modulesDir` at a directory
/// named `--`.
#[test]
fn a_setting_flag_never_claims_a_separator_or_another_flag() {
    for tokens in [
        ["--modules-dir", "--", "extra"].as_slice(),
        &["--modules-dir", "--"],
        &["--modules-dir", "--prod"],
        &["--modules-dir", "-C"],
    ] {
        let command_line = ["pacquet", "install"]
            .into_iter()
            .chain(tokens.iter().copied())
            .map(OsString::from)
            .collect::<Vec<_>>();
        let (overrides, remaining) = ConfigOverrides::extract(command_line.clone());
        assert_eq!(remaining, command_line, "{tokens:?}");

        let mut config = Config::default();
        overrides.apply(&mut config, Path::new("/workspace"));
        assert_eq!(config.modules_dir, Config::default().modules_dir, "{tokens:?}");
    }
}

/// A parsed setting decides for itself: `childConcurrency` reads a
/// negative value as "every core but this many", so the flag claims it
/// even though it opens with `-`.
#[test]
fn a_numeric_setting_claims_a_negative_value() {
    for tokens in [["--child-concurrency", "-1"].as_slice(), &["--child-concurrency=-1"]] {
        let command_line = ["pacquet", "install"]
            .into_iter()
            .chain(tokens.iter().copied())
            .map(OsString::from)
            .collect::<Vec<_>>();
        let (overrides, remaining) = ConfigOverrides::extract(command_line);
        assert_eq!(remaining, argv(["pacquet", "install"]), "{tokens:?}");

        let mut config = Config::default();
        overrides.apply(&mut config, Path::new("/workspace"));
        assert_eq!(
            config.child_concurrency,
            pnpm_config::resolve_child_concurrency(Some(-1)),
            "{tokens:?}",
        );
    }
}
