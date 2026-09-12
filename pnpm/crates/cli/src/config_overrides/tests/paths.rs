use super::{
    Config, ConfigOverrides, EnvVar, GetCurrentDir, GetHomeDir, LinkProbe, NodeLinker, Path,
    PathBuf, STORE_VERSION, apply_state_dir_override, apply_store_dir_override, argv, assert_eq,
};

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
