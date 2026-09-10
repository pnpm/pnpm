use super::{
    Config, EnvVar, EnvVarOs, GLOBAL_LAYOUT_VERSION, GetCurrentDir, GetHomeDir, Host, HostNoHome,
    LinkProbe, NPM_DEFAULT_REGISTRY, NodeLinker, NodePackageMapType, OsString, PackageImportMethod,
    Path, PathBuf, assert_eq, config_from_workspace_yaml, default_ci, default_state_dir,
    default_store_dir, fs, io, load_with_project_and_user, repo_on_branch, safe_host_var, tempdir,
    write_file,
};

#[test]
fn ci_false_disables_github_actions_detection() {
    struct GithubActionsWithCiFalse;

    impl EnvVar for GithubActionsWithCiFalse {
        fn var(name: &str) -> Option<String> {
            match name {
                "CI" => Some("false".to_string()),
                "GITHUB_ACTIONS" => Some("true".to_string()),
                _ => None,
            }
        }

        fn vars() -> Vec<(String, String)> {
            Vec::new()
        }
    }

    assert!(!default_ci::<GithubActionsWithCiFalse>(|| true));
}

#[test]
fn ci_detection_uses_injected_detector() {
    struct InjectedCi;

    impl EnvVar for InjectedCi {
        fn var(_: &str) -> Option<String> {
            None
        }

        fn vars() -> Vec<(String, String)> {
            Vec::new()
        }
    }

    assert!(default_ci::<InjectedCi>(|| true));
    assert!(!default_ci::<InjectedCi>(|| false));
}

#[test]
pub fn have_default_values() {
    let value = Config::new();
    assert_eq!(value.node_linker, NodeLinker::default());
    assert!(!value.node_experimental_package_map);
    assert_eq!(value.node_package_map_type, NodePackageMapType::Standard);
    assert_eq!(value.package_import_method, PackageImportMethod::default());
    assert!(value.prefer_frozen_lockfile);
    assert!(value.symlink);
    assert!(value.hoist);
    // The SmartDefault expression for `store_dir` resolves to
    // `default_store_dir::<Host>()` directly (no wrapper), so
    // calling the generic helper here with the same `Host`
    // capability must produce the same value — even on a developer
    // machine with `PNPM_HOME` / `XDG_DATA_HOME` set. This is the
    // wiring assertion that proves the SmartDefault field still
    // goes through the production capability; the per-branch
    // behaviour of `default_store_dir` is exercised with fake-`Sys`
    // structs in `defaults::tests`.
    assert_eq!(value.store_dir, default_store_dir::<Host>());
    assert_eq!(value.state_dir, default_state_dir::<Host>().unwrap_or_default());
    assert_eq!(value.registry, "https://registry.npmjs.org/");
}

#[test]
pub fn global_dirs_expand_a_leading_tilde() {
    let home = tempdir().expect("home tempdir");
    static HOME_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    HOME_PATH.set(home.path().to_path_buf()).expect("set once");
    let config_dir = home.path().join("xdg").join("pnpm");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(config_dir.join("config.yaml"), "globalDir: ~/global\nglobalBinDir: ~/bin\n")
        .expect("write global config.yaml");

    struct HostWithHome;
    impl EnvVar for HostWithHome {
        fn var(name: &str) -> Option<String> {
            if name == "XDG_CONFIG_HOME" {
                let xdg = HOME_PATH.get().expect("home path").join("xdg");
                return Some(xdg.to_str().expect("utf-8 home path").to_string());
            }
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithHome {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithHome {
        fn home_dir() -> Option<PathBuf> {
            HOME_PATH.get().cloned()
        }
    }
    inert_link_probe!(HostWithHome);
    host_current_dir!(HostWithHome);

    let project = tempdir().expect("project tempdir");
    let config =
        Config::new().current::<HostWithHome>(project.path()).expect("global config.yaml loads");
    assert_eq!(config.global_pkg_dir, Some(home.path().join("global").join(GLOBAL_LAYOUT_VERSION)));
    assert_eq!(config.global_bin, Some(home.path().join("bin")));
}

#[test]
pub fn fetch_retries_defaults_match_pnpm() {
    let value = Config::new();
    assert_eq!(value.fetch_retries, 2);
    assert_eq!(value.fetch_retry_factor, 10);
    assert_eq!(value.fetch_retry_mintimeout, 10_000);
    assert_eq!(value.fetch_retry_maxtimeout, 60_000);
}

#[test]
fn retry_options_use_the_resolved_network_budget() {
    let config = Config {
        fetch_retries: 4,
        fetch_retry_factor: 3,
        fetch_retry_mintimeout: 17,
        fetch_retry_maxtimeout: 91,
        ..Config::default()
    };
    let policy = config.retry_opts();
    assert_eq!(policy.retries, 4);
    assert_eq!(policy.factor, 3);
    assert_eq!(policy.min_timeout, std::time::Duration::from_millis(17));
    assert_eq!(policy.max_timeout, std::time::Duration::from_millis(91));
}

/// The two `noProxy` spellings are separate yaml keys, so an empty
/// primary must not consume the alias's turn.
/// A flag value is not a typed scalar, so `false` is a hostname on the
/// command line even though it disables proxying in an `.npmrc`.
#[test]
pub fn cli_proxy_flags_take_false_as_a_hostname() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    set_fake_env(&[("HTTPS_PROXY", "http://env-proxy.example.com:8080")]);

    let mut config = load_with_fake_env(project.path());
    config.apply_proxy_cli_overrides(Some("false"), None, None);

    assert_eq!(config.proxy.https_proxy.as_deref(), Some("false"));
}

/// Explicitly URL-scoped credentials pass through unchanged — they
/// are never rescoped, so they stay on exactly the registry the user
/// wrote, regardless of a workspace registry override.
#[test]
pub fn explicit_url_scoped_creds_pass_through() {
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    write_file(
        &user_file,
        "registry=https://trusted.example.com/\n//trusted.example.com/:_authToken=user-secret\n",
    );

    let config = load_with_project_and_user("registry=https://attacker.example.com/\n", user_file);

    assert_eq!(
        config.auth_headers.for_url("https://trusted.example.com/pkg").as_deref(),
        Some("Bearer user-secret"),
    );
    assert_eq!(config.auth_headers.for_url("https://attacker.example.com/pkg"), None);
}

#[test]
pub fn test_current_folder_fallback_to_default() {
    let current_dir = tempdir().unwrap();
    // Home dir is supplied but contains no `.npmrc`, so the
    // fallback to the caller-supplied default Config (the
    // `symlink: false` override) is what surfaces.
    let home_dir = tempdir().unwrap();
    static HOME_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    HOME_PATH.set(home_dir.path().to_path_buf()).expect("set once");
    struct HostWithHome;
    impl EnvVar for HostWithHome {
        fn var(name: &str) -> Option<String> {
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithHome {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithHome {
        fn home_dir() -> Option<PathBuf> {
            HOME_PATH.get().cloned()
        }
    }
    inert_link_probe!(HostWithHome);
    host_current_dir!(HostWithHome);
    let config = Config { symlink: false, ..Config::new() }
        .current::<HostWithHome>(current_dir.path())
        .expect("workspace yaml absent => no error");
    assert!(!config.symlink);
}

/// Both spellings reach one field, so which of them applies last decides.
#[test]
pub fn virtual_store_type_supersedes_the_boolean_spelling() {
    for (yaml, expected) in [
        ("virtualStoreType: project\n", false),
        ("virtualStoreType: global\n", true),
        ("enableGlobalVirtualStore: false\n", false),
        ("enableGlobalVirtualStore: true\n", true),
        ("virtualStoreType: project\nenableGlobalVirtualStore: true\n", false),
        ("virtualStoreType: global\nenableGlobalVirtualStore: false\n", true),
    ] {
        let tmp = tempdir().unwrap();
        fs::write(tmp.path().join("pnpm-workspace.yaml"), yaml)
            .expect("write to pnpm-workspace.yaml");
        let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
        assert_eq!(config.enable_global_virtual_store, expected, "yaml: {yaml}");
    }
}

#[test]
pub fn gvs_disabled_keeps_project_local_virtual_store() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "enableGlobalVirtualStore: false\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert!(!config.enable_global_virtual_store);
    assert_eq!(config.virtual_store_dir, tmp.path().join("node_modules/.pnpm"));
    assert_eq!(config.global_virtual_store_dir, config.store_dir.links());
}

#[test]
pub fn gvs_user_pinned_virtual_store_routes_into_global_virtual_store_dir() {
    let tmp = tempdir().unwrap();
    let user_path = tmp.path().join("custom-links");
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        format!("enableGlobalVirtualStore: true\nvirtualStoreDir: {}\n", user_path.display()),
    )
    .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert!(config.enable_global_virtual_store);
    assert_eq!(config.virtual_store_dir, user_path);
    assert_eq!(config.global_virtual_store_dir, user_path);
}

/// A single-project install (no `pnpm-workspace.yaml` anywhere)
/// keeps the CLI `--dir` as the anchor. Guards against the
/// re-anchor block accidentally firing when no workspace exists.
///
/// [`HostNoHome`] already pins the `NPM_CONFIG_WORKSPACE_DIR`
/// lookup to `None`, so the test never reads the host's real
/// environment.
#[test]
pub fn single_project_anchors_modules_at_cwd() {
    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("config loads");
    assert_eq!(config.modules_dir, tmp.path().join("node_modules"));
    assert_eq!(config.virtual_store_dir, tmp.path().join("node_modules/.pnpm"));
}

/// `scriptShell` is path-resolved only when it comes from the workspace
/// manifest. Global config and `PNPM_CONFIG_*` are machine-level sources, so
/// their path-like values stay raw just as pnpm's `manifestDir: undefined`
/// path does. Exercise the complete `Config::current` cascade rather than the
/// settings helper in isolation.
#[test]
pub fn script_shell_source_routing_matches_pnpm() {
    fake_env!(load_with_fake_env);
    let xdg = tempdir().expect("xdg tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(config_dir.join("config.yaml"), "scriptShell: ./global-shell.sh\n")
        .expect("write global config.yaml");

    let global_only = tempdir().expect("global-only project tempdir");
    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);
    let config = load_with_fake_env(global_only.path());
    assert_eq!(config.script_shell.as_deref(), Some("./global-shell.sh"));

    let workspace = tempdir().expect("workspace tempdir");
    fs::write(workspace.path().join("pnpm-workspace.yaml"), "scriptShell: ./workspace-shell.sh\n")
        .expect("write workspace yaml");
    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);
    let config = load_with_fake_env(workspace.path());
    let expected_workspace_shell =
        pnpm_fs::lexical_normalize(&workspace.path().join("workspace-shell.sh"))
            .to_string_lossy()
            .into_owned();
    assert_eq!(config.script_shell.as_deref(), Some(expected_workspace_shell.as_str()));

    set_fake_env(&[
        ("XDG_CONFIG_HOME", xdg.path().to_str().unwrap()),
        ("PNPM_CONFIG_SCRIPT_SHELL", "./env-shell.sh"),
    ]);
    let config = load_with_fake_env(workspace.path());
    assert_eq!(config.script_shell.as_deref(), Some("./env-shell.sh"));
}

/// npm spells the setting `maxsockets`; pnpm reads either spelling, so a
/// config carried over from npm keeps working.
#[test]
pub fn max_sockets_accepts_npms_lowercase_spelling() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "maxsockets: 7\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert_eq!(config.max_sockets, Some(7));
}

#[test]
fn resolved_minimum_release_age_treats_zero_as_disabled() {
    let mut config = Config::new();
    assert_eq!(config.resolved_minimum_release_age(), Some(1440), "default is 1 day");
    config.minimum_release_age = Some(0);
    assert_eq!(config.resolved_minimum_release_age(), None, "0 disables the cutoff");
    config.minimum_release_age = Some(60);
    assert_eq!(config.resolved_minimum_release_age(), Some(60));
    config.minimum_release_age = None;
    assert_eq!(config.resolved_minimum_release_age(), None);
}

#[test]
fn the_built_in_release_age_default_leaves_strict_mode_off() {
    let config = config_from_workspace_yaml("packages:\n  - '.'\n");

    assert_eq!(config.resolved_minimum_release_age(), Some(1440));
    assert!(!config.resolved_minimum_release_age_strict());
}

/// A cutoff the user typed turns on strict mode even when it repeats the
/// built-in default, so the setting gates the install instead of only
/// reporting it. Regression test for
/// <https://github.com/pnpm/pnpm/issues/14409>.
#[test]
fn an_explicit_release_age_turns_on_strict_mode() {
    let config = config_from_workspace_yaml("minimumReleaseAge: 1440\n");

    assert_eq!(config.minimum_release_age, Config::new().minimum_release_age);
    assert!(config.resolved_minimum_release_age_strict());
}

/// With no trusted user-level registry configured, package-manager
/// bootstrap falls back to the public npm registry — never the project's
/// attacker-controlled registry.
#[test]
pub fn package_manager_bootstrap_defaults_to_npm_registry() {
    let project = tempdir().expect("project tempdir");
    write_file(&project.path().join(".npmrc"), "registry=https://attacker.example.com/\n");
    let config = Config::default().current::<HostNoHome>(project.path()).expect("load config");

    assert_eq!(config.registry, "https://attacker.example.com/");
    assert_eq!(config.package_manager_bootstrap.registry, NPM_DEFAULT_REGISTRY);
}

/// A directly-constructed `PackageManagerBootstrap` (one not finalized
/// through `Config::current`) still defaults to the public npm registry,
/// never an empty registry the resolver would choke on.
#[test]
pub fn package_manager_bootstrap_default_registry_is_npm() {
    assert_eq!(crate::PackageManagerBootstrap::default().registry, NPM_DEFAULT_REGISTRY);
}

#[test]
pub fn the_branch_pattern_decides_merging_for_the_current_branch() {
    let repo = repo_on_branch("ref: refs/heads/release/1.0.0\n");
    fs::write(
        repo.path().join("pnpm-workspace.yaml"),
        "mergeGitBranchLockfilesBranchPattern:\n  - main\n  - release/*\n",
    )
    .unwrap();
    static REPO_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    REPO_DIR.set(repo.path().to_path_buf()).expect("set once");
    host_in_repo!(HostOnRelease);

    let config = Config::new().current::<HostOnRelease>(repo.path()).expect("yaml is valid");
    assert!(config.merge_git_branch_lockfiles);
}

#[test]
pub fn the_branch_pattern_leaves_an_unmatched_branch_alone() {
    let repo = repo_on_branch("ref: refs/heads/develop\n");
    fs::write(
        repo.path().join("pnpm-workspace.yaml"),
        "mergeGitBranchLockfilesBranchPattern:\n  - main\n  - release/*\n",
    )
    .unwrap();
    static REPO_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    REPO_DIR.set(repo.path().to_path_buf()).expect("set once");
    host_in_repo!(HostOnDevelop);

    let config = Config::new().current::<HostOnDevelop>(repo.path()).expect("yaml is valid");
    assert!(!config.merge_git_branch_lockfiles);
}
