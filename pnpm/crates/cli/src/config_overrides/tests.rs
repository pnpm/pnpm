use super::{ConfigOverrides, apply_registry_override, apply_store_dir_override};
use pacquet_config::{Config, EnvVar, GetCurrentDir, GetHomeDir, LinkProbe, NodeLinker};
use pacquet_store_dir::STORE_VERSION;
use pretty_assertions::assert_eq;
use std::{ffi::OsString, path::PathBuf};

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
    overrides.apply(&mut config);
    assert_eq!(config.registry, "https://example.test/");
    assert_eq!(config.package_manager_bootstrap.registry, "https://example.test/");
    assert_eq!(config.registries.get("default").map(String::as_str), Some("https://example.test/"));
    assert_eq!(
        config.package_manager_bootstrap.registries.get("default").map(String::as_str),
        Some("https://example.test/"),
    );
}

#[test]
fn registry_cli_override_normalizes_and_sets_every_registry_slot() {
    let mut config = Config::default();
    // No trailing slash on the input; it is normalized on the way in.
    apply_registry_override(&mut config, "https://cli.example");
    assert_eq!(config.registry, "https://cli.example/");
    assert_eq!(config.registries.get("default").map(String::as_str), Some("https://cli.example/"));
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
    overrides.apply(&mut config);
    assert_eq!(
        config.registries.get("@private").map(String::as_str),
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
    config.registries.insert("@private".to_string(), "https://workspace.example/npm/".to_string());
    config
        .package_manager_bootstrap
        .registries
        .insert("@private".to_string(), "https://json-env.example/npm/".to_string());
    overrides.apply(&mut config);
    assert_eq!(
        config.registries.get("@private").map(String::as_str),
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
    overrides.apply(&mut config);
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
    overrides.apply(&mut config);
    assert!(config.inject_workspace_packages);
    assert_eq!(config.node_linker, NodeLinker::Hoisted);
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
    overrides.apply(&mut config);
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
    overrides.apply(&mut config);
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
    overrides.apply(&mut config);
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
    overrides.apply(&mut config);

    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://proxy.example:8443"));
    assert_eq!(config.proxy.http_proxy.as_deref(), Some("http://proxy.example:8080"));
    assert_eq!(
        config.proxy.no_proxy,
        Some(pacquet_network::NoProxySetting::List(vec![
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
    overrides.apply(&mut config);
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
