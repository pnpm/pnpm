use super::{
    Config, EnvVar, EnvVarOs, GLOBAL_LAYOUT_VERSION, GetCurrentDir, GetHomeDir, HostNoHome,
    LinkProbe, LoadWorkspaceYamlError, OsString, Path, PathBuf, assert_eq, capture_warnings,
    default_store_dir, display_store_dir, fs, io, safe_host_var, tempdir, write_file,
};

#[test]
pub fn state_dir_uses_only_trusted_config_sources() {
    fake_env!(load_with_fake_env);
    let xdg = tempdir().expect("xdg tempdir");
    let config_dir = xdg.path().join("pnpm");
    let state_root = xdg.path().join("state");
    let resolved_state_root = dunce::canonicalize(xdg.path()).unwrap().join("state");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(config_dir.join("config.yaml"), "stateDir: from-global\n")
        .expect("write global config.yaml");

    let project = tempdir().expect("project tempdir");
    fs::write(project.path().join("pnpm-workspace.yaml"), "stateDir: from-project\n")
        .expect("write workspace yaml");

    set_fake_env(&[
        ("XDG_CONFIG_HOME", xdg.path().to_str().unwrap()),
        ("XDG_STATE_HOME", state_root.to_str().unwrap()),
    ]);
    let config = load_with_fake_env(project.path());
    assert_eq!(config.state_dir, resolved_state_root.join("from-global"));
    assert_eq!(config.workspace_key_issues.refused, ["stateDir"]);

    set_fake_env(&[
        ("XDG_CONFIG_HOME", xdg.path().to_str().unwrap()),
        ("XDG_STATE_HOME", state_root.to_str().unwrap()),
        ("PNPM_CONFIG_STATE_DIR", "from-env"),
    ]);
    let config = load_with_fake_env(project.path());
    assert_eq!(config.state_dir, resolved_state_root.join("from-env"));
    assert_eq!(
        config.explicit_settings.get("stateDir"),
        Some(&serde_json::Value::String("from-env".to_string())),
    );

    set_fake_env(&[
        ("XDG_CONFIG_HOME", xdg.path().to_str().unwrap()),
        ("XDG_STATE_HOME", state_root.to_str().unwrap()),
        ("PNPM_CONFIG_STATE_DIR", "../outside"),
    ]);
    let config = load_with_fake_env(project.path());
    assert!(config.state_dir.as_os_str().is_empty());
}

#[test]
pub fn global_dirs_use_only_trusted_config_sources() {
    fake_env!(load_with_fake_env);
    let xdg = tempdir().expect("xdg tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(
        config_dir.join("config.yaml"),
        "globalDir: from-global\nglobalBinDir: from-global-bin\n",
    )
    .expect("write global config.yaml");

    let project = tempdir().expect("project tempdir");
    fs::write(
        project.path().join("pnpm-workspace.yaml"),
        "globalDir: from-project\nglobalBinDir: from-project-bin\n",
    )
    .expect("write workspace yaml");

    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);
    let config = load_with_fake_env(project.path());
    assert_eq!(
        config.global_pkg_dir,
        Some(project.path().join("from-global").join(GLOBAL_LAYOUT_VERSION)),
    );
    assert_eq!(config.global_bin, Some(project.path().join("from-global-bin")));
    assert_eq!(config.workspace_key_issues.refused, ["globalDir", "globalBinDir"]);

    set_fake_env(&[
        ("XDG_CONFIG_HOME", xdg.path().to_str().unwrap()),
        ("PNPM_CONFIG_GLOBAL_DIR", "from-env"),
        ("PNPM_CONFIG_GLOBAL_BIN_DIR", "from-env-bin"),
    ]);
    let config = load_with_fake_env(project.path());
    assert_eq!(
        config.global_pkg_dir,
        Some(project.path().join("from-env").join(GLOBAL_LAYOUT_VERSION)),
    );
    assert_eq!(config.global_bin, Some(project.path().join("from-env-bin")));
    assert_eq!(
        config.explicit_settings.get("globalBinDir"),
        Some(&serde_json::Value::String("from-env-bin".to_string())),
    );
}

#[test]
pub fn network_settings_defaults_match_pnpm() {
    let value = Config::new();
    assert_eq!(value.network_concurrency, pnpm_network::default_network_concurrency());
    assert_eq!(value.fetch_timeout, 60_000);
    assert_eq!(value.fetch_warn_timeout_ms, 10_000);
    assert_eq!(value.fetch_min_speed_ki_bps, 50);
    assert!(value.user_agent.starts_with("pnpm/"), "user-agent: {:?}", value.user_agent);
    assert_eq!(value.npmrc_auth_file, None);
}

#[test]
pub fn network_settings_maps_custom_config_values() {
    let mut config = Config::new();
    config.network_concurrency = 8;
    config.fetch_timeout = 120_000;
    config.fetch_warn_timeout_ms = 2_345;
    config.fetch_min_speed_ki_bps = 12;
    config.user_agent = "pnpm-test".to_string();

    let settings = config.network_settings();
    assert_eq!(settings.network_concurrency, 8);
    assert_eq!(settings.fetch_timeout, std::time::Duration::from_mins(2));
    assert_eq!(settings.fetch_warn_timeout, std::time::Duration::from_millis(2_345));
    assert_eq!(settings.fetch_min_speed_ki_bps, 12);
    assert_eq!(settings.user_agent, "pnpm-test");
}

#[test]
pub fn global_config_yaml_request_destination_values_expand_env() {
    fake_env!(load_with_fake_env);
    let xdg = tempdir().expect("xdg tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(
        config_dir.join("config.yaml"),
        r"
registry: https://${REGISTRY_HOST}/npm/
pnprServer: https://${REGISTRY_HOST}/pnpr/
namedRegistries:
  work: https://${REGISTRY_HOST}/work/
",
    )
    .expect("write global config.yaml");

    let project = tempdir().expect("project tempdir");
    set_fake_env(&[
        ("REGISTRY_HOST", "trusted.example.com"),
        ("XDG_CONFIG_HOME", xdg.path().to_str().unwrap()),
    ]);
    let config = load_with_fake_env(project.path());

    assert_eq!(config.registry, "https://trusted.example.com/npm/");
    assert_eq!(config.pnpr_server.as_deref(), Some("https://trusted.example.com/pnpr/"));
    assert_eq!(
        config.registries_by_prefix.get("work").map(String::as_str),
        Some("https://trusted.example.com/work/"),
    );
}

#[test]
pub fn pnpm_config_request_destinations_expand_env() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    set_fake_env(&[
        ("PNPM_CONFIG_PNPR_SERVER", "https://${REGISTRY_HOST}/pnpr/"),
        ("PNPM_CONFIG_REGISTRY", "https://${REGISTRY_HOST}/npm/"),
        ("REGISTRY_HOST", "env.example.com"),
    ]);
    let config = load_with_fake_env(project.path());

    assert_eq!(config.pnpr_server.as_deref(), Some("https://env.example.com/pnpr/"));
    assert_eq!(config.registry, "https://env.example.com/npm/");
}

/// End-to-end: malformed `pnpm_config__auth` JSON aborts the load with an
/// error rather than silently dropping the auth.
#[test]
pub fn json_env_malformed_json_aborts_the_load() {
    fake_env!();
    let project = tempdir().expect("project tempdir");
    set_fake_env(&[("pnpm_config__auth", "{ not valid json")]);

    let result = Config::default().current::<FakeEnv>(project.path());
    assert!(matches!(result, Err(LoadWorkspaceYamlError::InvalidJsonAuth { .. })));
}

/// End-to-end: a non-object top-level `pnpm_config__auth` aborts the load.
#[test]
pub fn json_env_non_object_top_level_aborts_the_load() {
    fake_env!();
    let project = tempdir().expect("project tempdir");
    set_fake_env(&[("pnpm_config__auth", r#"["array","is","not","an","object"]"#)]);

    let result = Config::default().current::<FakeEnv>(project.path());
    assert!(matches!(result, Err(LoadWorkspaceYamlError::InvalidJsonAuth { .. })));
}

/// End-to-end: the "@" (default) scope in `pnpm_config__auth` routes the
/// default registry to its host — `pnpm add <pkg>` resolves against the
/// env-declared host, not the npmjs default. Confirmed semantics in
/// pnpm/pnpm#12559.
#[test]
pub fn json_env_default_scope_routes_default_registry() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    set_fake_env(&[(
        "pnpm_config__auth",
        r#"{"https://my-npm-proxy.example":{"@":{"authToken":"proxy-token"}}}"#,
    )]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.registry, "https://my-npm-proxy.example/");
    assert_eq!(
        config.registries_by_scope.get("default").map(String::as_str),
        Some("https://my-npm-proxy.example/"),
    );
    assert_eq!(
        config.auth_headers.for_url("https://my-npm-proxy.example/pkg").as_deref(),
        Some("Bearer proxy-token"),
    );
}

/// End-to-end: a package scope in `pnpm_config__auth` routes that scope
/// to its host.
#[test]
pub fn json_env_scoped_entry_routes_that_scope() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    set_fake_env(&[(
        "pnpm_config__auth",
        r#"{"https://npm.pkg.github.com":{"@org":{"authToken":"org-token"}}}"#,
    )]);

    let config = load_with_fake_env(project.path());

    assert_eq!(
        config.registries_by_scope.get("@org").map(String::as_str),
        Some("https://npm.pkg.github.com/"),
    );
}

/// End-to-end: a `PNPM_CONFIG_REGISTRY` env var (CLI-equivalent) still
/// wins over the env JSON default — matching pnpm's "CLI > env JSON >
/// yaml" precedence.
#[test]
pub fn json_env_env_registry_flag_wins_over_json_env_default() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    set_fake_env(&[
        (
            "pnpm_config__auth",
            r#"{"https://my-npm-proxy.example":{"@":{"authToken":"proxy-token"}}}"#,
        ),
        ("PNPM_CONFIG_REGISTRY", "https://cli-registry.example/"),
    ]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.registry, "https://cli-registry.example/");
    assert_eq!(
        config.registries_by_scope.get("default").map(String::as_str),
        Some("https://cli-registry.example/"),
    );
    assert_eq!(
        config.package_manager_bootstrap.registries.get("default").map(String::as_str),
        Some("https://cli-registry.example/"),
    );
    // Token is still pinned to the env-declared host.
    assert_eq!(
        config.auth_headers.for_url("https://my-npm-proxy.example/pkg").as_deref(),
        Some("Bearer proxy-token"),
    );
}

/// End-to-end: env-inferred registry routes flow through to the
/// package-manager bootstrap path (self-download / version switching).
#[test]
pub fn json_env_inferred_registries_flow_to_bootstrap() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    set_fake_env(&[(
        "pnpm_config__auth",
        r#"{"https://my-npm-proxy.example":{"@":{"authToken":"proxy-token"},"@org":{"authToken":"org-token"}}}"#,
    )]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.package_manager_bootstrap.registry, "https://my-npm-proxy.example/");
    assert_eq!(
        config.package_manager_bootstrap.registries.get("@org").map(String::as_str),
        Some("https://my-npm-proxy.example/"),
    );
    assert_eq!(
        config
            .package_manager_bootstrap
            .auth_headers
            .for_url("https://my-npm-proxy.example/pkg")
            .as_deref(),
        Some("Bearer proxy-token"),
    );
    assert_eq!(
        config
            .package_manager_bootstrap
            .auth_headers
            .for_url_with_package("https://my-npm-proxy.example/org/foo", Some("@org/foo"))
            .as_deref(),
        Some("Bearer org-token"),
    );
}

/// Env JSON routes override user-level (`~/.npmrc` / `auth.ini`) scoped
/// registries in the package-manager bootstrap: env JSON outranks the
/// trusted `.npmrc`. CLI scoped overrides still win — applied later by
/// `ConfigOverrides`.
#[test]
pub fn json_env_overrides_user_bootstrap_scoped_registry() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    let auth = tempdir().expect("auth tempdir");
    let auth_file = auth.path().join("user-npmrc");
    write_file(&auth_file, "@org:registry=https://user-registry.example/\n");
    set_fake_env(&[
        ("PNPM_CONFIG_NPMRC_AUTH_FILE", auth_file.to_str().unwrap()),
        (
            "pnpm_config__auth",
            r#"{"https://my-npm-proxy.example":{"@org":{"authToken":"org-token"}}}"#,
        ),
    ]);

    let config = load_with_fake_env(project.path());

    assert_eq!(
        config.package_manager_bootstrap.registries.get("@org").map(String::as_str),
        Some("https://my-npm-proxy.example/"),
    );
    assert_eq!(
        config.registries_by_scope.get("@org").map(String::as_str),
        Some("https://my-npm-proxy.example/"),
    );
}

#[test]
pub fn global_config_yaml_supplies_proxy_settings() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    let xdg = tempdir().expect("config tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create global config dir");
    fs::write(
        config_dir.join("config.yaml"),
        "httpProxy: http://proxy.example.com:8080\n\
         httpsProxy: http://proxy.example.com:8443\n\
         noProxy: localhost,127.0.0.1\n",
    )
    .expect("write global config.yaml");

    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);
    let config = load_with_fake_env(project.path());

    assert_eq!(config.proxy.http_proxy.as_deref(), Some("http://proxy.example.com:8080"));
    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://proxy.example.com:8443"));
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
pub fn global_config_yaml_proxy_overrides_project_npmrc() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    fs::write(project.path().join(".npmrc"), "https-proxy=http://npmrc-proxy.example.com:8080\n")
        .expect("write project .npmrc");
    let xdg = tempdir().expect("config tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create global config dir");
    fs::write(config_dir.join("config.yaml"), "httpsProxy: http://yaml-proxy.example.com:9090\n")
        .expect("write global config.yaml");

    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);
    let config = load_with_fake_env(project.path());

    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://yaml-proxy.example.com:9090"));
    assert_eq!(config.proxy.http_proxy.as_deref(), Some("http://yaml-proxy.example.com:9090"));
}

#[test]
pub fn global_config_yaml_https_proxy_preserves_project_npmrc_http_proxy() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    fs::write(
        project.path().join(".npmrc"),
        "http-proxy=http://project-http-proxy.example.com:8080\n",
    )
    .expect("write project .npmrc");
    let xdg = tempdir().expect("config tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create global config dir");
    fs::write(config_dir.join("config.yaml"), "httpsProxy: http://yaml-proxy.example.com:9090\n")
        .expect("write global config.yaml");

    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);
    let config = load_with_fake_env(project.path());

    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://yaml-proxy.example.com:9090"));
    assert_eq!(
        config.proxy.http_proxy.as_deref(),
        Some("http://project-http-proxy.example.com:8080"),
    );
    assert_eq!(
        config.package_manager_bootstrap.proxy.http_proxy.as_deref(),
        Some("http://yaml-proxy.example.com:9090"),
    );
}

#[test]
pub fn pnpm_config_https_proxy_preserves_global_http_proxy() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    let xdg = tempdir().expect("config tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create global config dir");
    fs::write(
        config_dir.join("config.yaml"),
        "httpsProxy: http://yaml-proxy.example.com:9090\n\
         httpProxy: http://yaml-http-proxy.example.com:8080\n",
    )
    .expect("write global config.yaml");

    set_fake_env(&[
        ("XDG_CONFIG_HOME", xdg.path().to_str().unwrap()),
        ("PNPM_CONFIG_HTTPS_PROXY", "http://cli-proxy.example.com:7070"),
    ]);
    let config = load_with_fake_env(project.path());

    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://cli-proxy.example.com:7070"));
    assert_eq!(config.proxy.http_proxy.as_deref(), Some("http://yaml-http-proxy.example.com:8080"));
    assert_eq!(config.package_manager_bootstrap.proxy, config.proxy);
}

#[test]
pub fn project_npmrc_proxy_settings_are_preserved() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    fs::write(
        project.path().join(".npmrc"),
        "https-proxy=http://npmrc-proxy.example.com:8080\n\
         proxy=http://npmrc-http-proxy.example.com:3128\n\
         no-proxy=internal.example.com\n",
    )
    .expect("write project .npmrc");
    set_fake_env(&[]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://npmrc-proxy.example.com:8080"));
    assert_eq!(config.proxy.http_proxy.as_deref(), Some("http://npmrc-proxy.example.com:8080"));
    assert_eq!(
        config.proxy.no_proxy,
        Some(pnpm_network::NoProxySetting::List(vec!["internal.example.com".to_string()])),
    );
}

#[test]
pub fn cli_https_proxy_preserves_project_npmrc_http_proxy_only_for_project_requests() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    fs::write(
        project.path().join(".npmrc"),
        "http-proxy=http://project-http-proxy.example.com:8080\n",
    )
    .expect("write project .npmrc");
    set_fake_env(&[]);

    let mut config = load_with_fake_env(project.path());
    config.apply_proxy_cli_overrides(Some("http://cli-https-proxy.example.com:8443"), None, None);

    assert_eq!(
        config.proxy.http_proxy.as_deref(),
        Some("http://project-http-proxy.example.com:8080"),
    );
    assert_eq!(
        config.package_manager_bootstrap.proxy.http_proxy.as_deref(),
        Some("http://cli-https-proxy.example.com:8443"),
    );
}

#[test]
pub fn cli_https_proxy_preserves_trusted_npmrc_http_proxy_for_bootstrap_requests() {
    let project = tempdir().expect("project tempdir");
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    write_file(&user_file, "http-proxy=http://user-http-proxy.example.com:8080\n");

    let mut config = Config { npmrc_auth_file: Some(user_file), ..Config::default() }
        .current::<HostNoHome>(project.path())
        .expect("load config");
    config.apply_proxy_cli_overrides(Some("http://cli-https-proxy.example.com:8443"), None, None);

    assert_eq!(config.proxy.http_proxy.as_deref(), Some("http://user-http-proxy.example.com:8080"));
    assert_eq!(
        config.package_manager_bootstrap.proxy.http_proxy.as_deref(),
        Some("http://user-http-proxy.example.com:8080"),
    );
}

#[test]
pub fn cli_https_proxy_precedes_standard_http_proxy_environment_fallback() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    set_fake_env(&[("HTTP_PROXY", "http://environment-http-proxy.example.com:8080")]);

    let mut config = load_with_fake_env(project.path());
    config.apply_proxy_cli_overrides(Some("http://cli-https-proxy.example.com:8443"), None, None);

    assert_eq!(config.proxy.http_proxy.as_deref(), Some("http://cli-https-proxy.example.com:8443"));
    assert_eq!(config.package_manager_bootstrap.proxy, config.proxy);
}

/// A flag names its key even when the value reads as unset, so the
/// `.npmrc` below it cannot win the key back — the cascade falls through
/// to the other keys and the environment, and here to neither.
#[test]
pub fn empty_cli_proxy_flags_mask_the_project_npmrc() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join(".npmrc"),
        "https-proxy=http://npmrc-proxy.example.com:8443\nno-proxy=skip.example\n",
    );
    set_fake_env(&[]);

    let mut config = load_with_fake_env(project.path());
    config.apply_proxy_cli_overrides(Some(""), Some(""), Some(""));

    assert_eq!(config.proxy.https_proxy, None);
    assert_eq!(config.proxy.http_proxy, None);
    assert_eq!(config.proxy.no_proxy, None);
}

#[test]
pub fn empty_cli_proxy_flags_fall_through_to_the_environment() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(&project.path().join(".npmrc"), "https-proxy=http://npmrc-proxy.example.com:8443\n");
    set_fake_env(&[("HTTPS_PROXY", "http://env-proxy.example.com:8080")]);

    let mut config = load_with_fake_env(project.path());
    config.apply_proxy_cli_overrides(Some(""), None, None);

    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://env-proxy.example.com:8080"));
}

#[test]
pub fn empty_global_config_yaml_proxy_settings_mask_the_project_npmrc() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join(".npmrc"),
        "https-proxy=http://npmrc-proxy.example.com:8443\nno-proxy=skip.example\n",
    );
    let xdg = tempdir().expect("config tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create global config dir");
    fs::write(config_dir.join("config.yaml"), "httpsProxy: \"\"\nhttpProxy: \"\"\nnoProxy: \"\"\n")
        .expect("write global config.yaml");

    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);
    let config = load_with_fake_env(project.path());

    assert_eq!(config.proxy.https_proxy, None);
    assert_eq!(config.proxy.http_proxy, None);
    assert_eq!(config.proxy.no_proxy, None);
}

/// A project `.npmrc`'s own unscoped credentials warn too, named by the
/// path pnpm read them from.
#[test]
pub fn unscoped_creds_in_project_npmrc_warn_naming_that_file() {
    let auth = tempdir().expect("auth tempdir");
    let project = tempdir().expect("project tempdir");
    write_file(&project.path().join(".npmrc"), "registry=https://ws.example.com/\n_authToken=t\n");

    let warnings = capture_warnings(|| {
        drop(
            Config { npmrc_auth_file: Some(auth.path().join("user-npmrc")), ..Config::default() }
                .current::<HostNoHome>(project.path())
                .expect("load config"),
        );
    });

    let warning = warnings
        .iter()
        .find(|warning| warning.contains("Unscoped per-registry settings"))
        .expect("deprecation warning");
    assert!(
        warning.contains(&project.path().join(".npmrc").display().to_string()),
        "{warning:?} should name the project .npmrc",
    );
}

/// `default_store_dir`'s `PNPM_HOME` branch, exercised through the
/// generic capability seam — no process-environment mutation, no
/// `EnvGuard` lock, no `unsafe` block. With the DI seam from
/// pnpm/pacquet#339 + pnpm/pnpm#11708 the precedence is checked by
/// passing a per-test unit struct that satisfies [`EnvVar`],
/// [`GetHomeDir`], and [`GetCurrentDir`].
///
/// The `home_dir` and `current_dir` capability impls both call
/// `unreachable!` because `default_store_dir` short-circuits on
/// `PNPM_HOME` before consulting either — the panic-on-call
/// documents that precondition. Tracks pnpm/pacquet#343.
#[test]
pub fn should_use_pnpm_home_env_var() {
    struct EnvWithPnpmHome;
    impl EnvVar for EnvWithPnpmHome {
        fn var(name: &str) -> Option<String> {
            (name == "PNPM_HOME").then(|| "/hello".to_owned())
        }
    }
    impl GetHomeDir for EnvWithPnpmHome {
        fn home_dir() -> Option<PathBuf> {
            unreachable!("home_dir must not be called when PNPM_HOME is set");
        }
    }
    impl GetCurrentDir for EnvWithPnpmHome {
        fn current_dir() -> io::Result<PathBuf> {
            unreachable!("current_dir must not be called when PNPM_HOME is set");
        }
    }
    let store_dir = default_store_dir::<EnvWithPnpmHome>();
    assert_eq!(
        display_store_dir(&store_dir),
        format!("/hello/store/{}", pnpm_store_dir::STORE_VERSION),
    );
}

/// Companion to [`should_use_pnpm_home_env_var`]: when
/// `PNPM_HOME` is unset, `default_store_dir` falls through to
/// `XDG_DATA_HOME`. Exercised through the DI seam with a fake
/// `Sys` that only returns a value for `XDG_DATA_HOME`. No
/// process-environment mutation, no `EnvGuard`, no `unsafe`.
/// Tracks pnpm/pacquet#343.
#[test]
pub fn should_use_xdg_data_home_env_var() {
    struct EnvWithXdgDataHome;
    impl EnvVar for EnvWithXdgDataHome {
        fn var(name: &str) -> Option<String> {
            (name == "XDG_DATA_HOME").then(|| "/hello".to_owned())
        }
    }
    impl GetHomeDir for EnvWithXdgDataHome {
        fn home_dir() -> Option<PathBuf> {
            unreachable!("home_dir must not be called when XDG_DATA_HOME is set");
        }
    }
    impl GetCurrentDir for EnvWithXdgDataHome {
        fn current_dir() -> io::Result<PathBuf> {
            unreachable!("current_dir must not be called when XDG_DATA_HOME is set");
        }
    }
    let store_dir = default_store_dir::<EnvWithXdgDataHome>();
    assert_eq!(
        display_store_dir(&store_dir),
        format!("/hello/pnpm/store/{}", pnpm_store_dir::STORE_VERSION),
    );
}

#[test]
pub fn npmrc_in_current_folder_applies_registry() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join(".npmrc"), "registry=https://cwd.example").expect("write to .npmrc");
    let config =
        Config::new().current::<HostNoHome>(tmp.path()).expect("workspace yaml absent => no error");
    assert_eq!(config.registry, "https://cwd.example/");
}

/// pnpm 11 does not treat the `fetch-retries*` family as an
/// `.npmrc`-readable auth setting, so a value like `fetch-retries=99`
/// in `.npmrc` is silently ignored. pacquet must do the same —
/// applying it would silently change install behaviour for projects
/// that have a stale `.npmrc` lying around.
#[test]
pub fn fetch_retry_keys_in_npmrc_are_ignored() {
    let tmp = tempdir().unwrap();
    let ini = "fetch-retries=99\nfetch-retry-factor=99\nfetch-retry-mintimeout=99\nfetch-retry-maxtimeout=99\n";
    fs::write(tmp.path().join(".npmrc"), ini).expect("write to .npmrc");
    let defaults = Config::new();
    let config =
        Config::new().current::<HostNoHome>(tmp.path()).expect("workspace yaml absent => no error");
    assert_eq!(config.fetch_retries, defaults.fetch_retries);
    assert_eq!(config.fetch_retry_factor, defaults.fetch_retry_factor);
    assert_eq!(config.fetch_retry_mintimeout, defaults.fetch_retry_mintimeout);
    assert_eq!(config.fetch_retry_maxtimeout, defaults.fetch_retry_maxtimeout);
}

#[test]
pub fn test_current_folder_for_invalid_npmrc() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join(".npmrc"), b"Hello \xff World").expect("write to .npmrc");
    let config =
        Config::new().current::<HostNoHome>(tmp.path()).expect("workspace yaml absent => no error");
    assert!(config.symlink); // default — invalid .npmrc is silently ignored
}

#[test]
pub fn npmrc_in_home_folder_applies_registry() {
    let current_dir = tempdir().unwrap();
    let home_dir = tempdir().unwrap();
    fs::write(home_dir.path().join(".npmrc"), "registry=https://home.example")
        .expect("write to .npmrc");
    // Per-test fake: home_dir is a tempdir, so it can't be a
    // module-level constant — stash it in a per-test `OnceLock`
    // so `GetHomeDir::home_dir`'s associated-function shape (no
    // `&self`) can still resolve it at call time.
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
    let config = Config::new()
        .current::<HostWithHome>(current_dir.path())
        .expect("workspace yaml absent => no error");
    assert_eq!(config.registry, "https://home.example/");
}

#[test]
pub fn npmrc_scope_alone_never_reaches_the_config() {
    // pnpm's `.npmrc` reader keeps only auth/registry keys, so this is the one
    // config source that must *not* supply the login scope.
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join(".npmrc"), "scope=@from-npmrc\n").expect("write to .npmrc");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("config loads");
    assert_eq!(config.scope, None);
}

/// A `Config` that never goes through [`Config::current`] never runs
/// [`Config::apply_global_virtual_store_derivation`] either, so the
/// `SmartDefault` has to hold the same invariant on its own: the
/// machine-wide store never points at the working directory.
#[test]
pub fn default_config_disables_gvs_and_points_it_at_the_store() {
    let config = Config::default();
    assert!(!config.enable_global_virtual_store);
    assert_eq!(config.global_virtual_store_dir, config.store_dir.links());
}
