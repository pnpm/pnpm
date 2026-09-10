use super::{
    Config, EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, HostNoHome, LinkProbe, OsString, Path,
    PathBuf, assert_eq, fs, io, safe_host_var, tempdir, write_file,
};

/// End-to-end: the env-inferred default registry wins over a
/// repo-controlled `pnpm-workspace.yaml` default. The credential and
/// its destination host come from the same trusted env value, so yaml
/// cannot redirect the env token to a different registry.
#[test]
pub fn json_env_env_default_wins_over_workspace_yaml_default() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join("pnpm-workspace.yaml"),
        "registries:\n  default: https://registry.npmjs.org/\n",
    );
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
}

/// End-to-end: a scoped env JSON entry overrides a repo-controlled
/// `pnpm-workspace.yaml` scoped registry in the main cascade, and the
/// token is pinned to the env-declared host. Asserts
/// `config.registries_by_scope["@scope"]` — not just auth headers — so a regression
/// that breaks routing while leaving auth-header pinning intact is caught.
#[test]
pub fn json_env_env_scoped_wins_over_workspace_yaml_scoped() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join("pnpm-workspace.yaml"),
        "registries:\n  '@victim-scope': https://attacker.example/\n",
    );
    set_fake_env(&[(
        "pnpm_config__auth",
        r#"{"https://registry.npmjs.org":{"@victim-scope":{"authToken":"secret-token"}}}"#,
    )]);

    let config = load_with_fake_env(project.path());

    assert_eq!(
        config.registries_by_scope.get("@victim-scope").map(String::as_str),
        Some("https://registry.npmjs.org/"),
    );
    assert_eq!(
        config
            .auth_headers
            .for_url_with_package(
                "https://registry.npmjs.org/@victim-scope/foo",
                Some("@victim-scope/foo")
            )
            .as_deref(),
        Some("Bearer secret-token"),
    );
    assert!(
        config.auth_headers.for_url("https://attacker.example/@victim-scope/foo").is_none(),
        "repo-controlled registry URL must not receive the env token",
    );
}

/// A workspace `.npmrc`'s own unscoped credential pins to the
/// workspace registry (the project file is the highest-priority
/// source, and its creds scope to its own registry).
#[test]
pub fn workspace_unscoped_creds_pin_to_workspace_registry() {
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join(".npmrc"),
        "registry=https://workspace.example.com/\n_authToken=workspace-token\n",
    );
    let config = Config::default().current::<HostNoHome>(project.path()).expect("load config");
    assert_eq!(
        config.auth_headers.for_url("https://workspace.example.com/pkg").as_deref(),
        Some("Bearer workspace-token"),
    );
}

#[test]
pub fn workspace_yaml_proxy_is_not_trusted_for_package_manager_bootstrap() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    fs::write(
        project.path().join("pnpm-workspace.yaml"),
        "httpsProxy: http://workspace-proxy.example.com:9090\n",
    )
    .expect("write pnpm-workspace.yaml");
    let xdg = tempdir().expect("config tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create global config dir");
    fs::write(
        config_dir.join("config.yaml"),
        "httpsProxy: http://trusted-proxy.example.com:8080\n\
         httpProxy: http://trusted-http-proxy.example.com:8080\n",
    )
    .expect("write global config.yaml");

    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);
    let config = load_with_fake_env(project.path());

    assert_eq!(
        config.proxy.https_proxy.as_deref(),
        Some("http://workspace-proxy.example.com:9090"),
    );
    assert_eq!(
        config.proxy.http_proxy.as_deref(),
        Some("http://trusted-http-proxy.example.com:8080"),
    );
    assert_eq!(
        config.package_manager_bootstrap.proxy.https_proxy.as_deref(),
        Some("http://trusted-proxy.example.com:8080"),
    );
    assert_eq!(
        config.package_manager_bootstrap.proxy.http_proxy.as_deref(),
        Some("http://trusted-http-proxy.example.com:8080"),
    );
}

/// A yaml key set to an unset-reading value still masks the `.npmrc`
/// below it, so nothing is left for the cascade to fall through to.
#[test]
pub fn empty_workspace_yaml_proxy_settings_mask_the_project_npmrc() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join(".npmrc"),
        "https-proxy=http://npmrc-proxy.example.com:8443\nno-proxy=skip.example\n",
    );
    fs::write(
        project.path().join("pnpm-workspace.yaml"),
        "httpsProxy: \"\"\nhttpProxy: \"\"\nproxy: \"\"\nnoProxy: \"\"\n",
    )
    .expect("write pnpm-workspace.yaml");
    set_fake_env(&[]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.proxy.https_proxy, None);
    assert_eq!(config.proxy.http_proxy, None);
    assert_eq!(config.proxy.no_proxy, None);
}

#[test]
pub fn workspace_yaml_proxy_false_disables_an_environment_proxy() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    fs::write(project.path().join("pnpm-workspace.yaml"), "proxy: false\n")
        .expect("write pnpm-workspace.yaml");
    set_fake_env(&[("HTTPS_PROXY", "http://env-proxy.example.com:8080")]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.proxy.https_proxy, None);
    assert_eq!(config.proxy.http_proxy, None);
}

/// `proxy` and `https-proxy` are separate keys, and the more specific one
/// wins regardless of which file set it — so a repo yaml turning proxying
/// off does not drop the user's `https-proxy`.
#[test]
pub fn workspace_yaml_proxy_false_yields_to_an_npmrc_https_proxy() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(&project.path().join(".npmrc"), "https-proxy=http://npmrc-proxy.example.com:8443\n");
    fs::write(project.path().join("pnpm-workspace.yaml"), "proxy: false\n")
        .expect("write pnpm-workspace.yaml");
    set_fake_env(&[]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://npmrc-proxy.example.com:8443"));
    assert_eq!(config.proxy.http_proxy.as_deref(), Some("http://npmrc-proxy.example.com:8443"));
}

#[test]
pub fn workspace_yaml_proxy_false_overrides_an_npmrc_legacy_proxy() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(&project.path().join(".npmrc"), "proxy=http://npmrc-legacy.example.com:8443\n");
    fs::write(project.path().join("pnpm-workspace.yaml"), "proxy: false\n")
        .expect("write pnpm-workspace.yaml");
    set_fake_env(&[]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.proxy.https_proxy, None);
    assert_eq!(config.proxy.http_proxy, None);
}

#[test]
pub fn workspace_yaml_scheme_proxy_keys_set_to_false_mask_the_project_npmrc() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join(".npmrc"),
        "https-proxy=http://npmrc-proxy.example.com:8443\nno-proxy=skip.example\n",
    );
    fs::write(
        project.path().join("pnpm-workspace.yaml"),
        "httpsProxy: false\nhttpProxy: false\nnoProxy: false\n",
    )
    .expect("write pnpm-workspace.yaml");
    set_fake_env(&[("NO_PROXY", "env.example")]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.proxy.https_proxy, None);
    assert_eq!(config.proxy.http_proxy, None);
    assert_eq!(
        config.proxy.no_proxy,
        Some(pnpm_network::NoProxySetting::List(vec!["env.example".to_string()])),
        "a masked key still falls through to the environment",
    );
}

#[test]
pub fn empty_workspace_yaml_no_proxy_falls_through_to_its_alias() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    fs::write(
        project.path().join("pnpm-workspace.yaml"),
        "noProxy: \"\"\nnoproxy: alias.example\n",
    )
    .expect("write pnpm-workspace.yaml");
    set_fake_env(&[]);

    let config = load_with_fake_env(project.path());

    assert_eq!(
        config.proxy.no_proxy,
        Some(pnpm_network::NoProxySetting::List(vec!["alias.example".to_string()])),
    );
}

#[test]
pub fn pnpm_workspace_yaml_registry_overrides_npmrc_registry() {
    // `registry` is the one non-scope key pnpm 11 still reads from
    // .npmrc. When both files define it, the yaml wins, matching
    // pnpm itself.
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join(".npmrc"), "registry=https://from-npmrc.test")
        .expect("write to .npmrc");
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "registry: https://from-yaml.test\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert_eq!(config.registry, "https://from-yaml.test/");
}

/// See [`crate::refused_keys`] for why a repository-committed file must not be
/// able to choose the login scope.
#[test]
pub fn pnpm_workspace_yaml_cannot_supply_the_login_scope() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "scope: '@from-yaml'\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert_eq!(config.scope, None);
    assert_eq!(config.workspace_key_issues.refused, vec!["scope".to_owned()]);
}

#[test]
pub fn global_config_yaml_supplies_the_login_scope_over_workspace_yaml() {
    fake_env!(load_with_fake_env);
    let xdg = tempdir().expect("xdg tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(config_dir.join("config.yaml"), "scope: '@from-global-config'\n")
        .expect("write global config.yaml");

    let project = tempdir().expect("project tempdir");
    fs::write(project.path().join("pnpm-workspace.yaml"), "scope: '@from-yaml'\n")
        .expect("write pnpm-workspace.yaml");
    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.scope.as_deref(), Some("@from-global-config"));
}

#[test]
pub fn pnpm_config_scope_env_var_overrides_the_login_scope_in_workspace_yaml() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    fs::write(project.path().join("pnpm-workspace.yaml"), "scope: '@from-yaml'\n")
        .expect("write pnpm-workspace.yaml");
    set_fake_env(&[("PNPM_CONFIG_SCOPE", "@from-env")]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.scope.as_deref(), Some("@from-env"));
}

#[test]
pub fn pnpm_workspace_yaml_found_by_walking_up() {
    let tmp = tempdir().unwrap();
    let nested = tmp.path().join("packages/inner");
    fs::create_dir_all(&nested).unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "symlink: false\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(&nested).expect("yaml is valid");
    assert!(!config.symlink);
}

#[test]
pub fn workspace_subdir_reads_workspace_root_npmrc() {
    let tmp = tempdir().unwrap();
    let nested = tmp.path().join("packages/web");
    fs::create_dir_all(&nested).unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write to pnpm-workspace.yaml");
    fs::write(tmp.path().join(".npmrc"), "registry=https://workspace-npmrc.example/\n")
        .expect("write to .npmrc");

    let config = Config::new().current::<HostNoHome>(&nested).expect("config loads");

    assert_eq!(config.registry, "https://workspace-npmrc.example/");
}

#[test]
pub fn gvs_enabled_exposes_hoisted_dependencies_through_node_path_and_the_esm_loader() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "enableGlobalVirtualStore: true\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    let path_delimiter = if cfg!(windows) { ";" } else { ":" };
    assert_eq!(
        config.extra_env.get("NODE_PATH"),
        Some(&format!(
            "{}{path_delimiter}{}",
            tmp.path().join("node_modules").join(".pnpm").join("node_modules").display(),
            tmp.path().join("node_modules").display(),
        )),
    );
    let node_options = config.extra_env.get("NODE_OPTIONS").expect("NODE_OPTIONS is injected");
    assert!(node_options.contains(crate::esm_node_path_loader::esm_node_path_loader_import_flag()));
}

/// Run from a workspace package, `NODE_PATH` must point at the
/// workspace root's virtual store — the one that exists — not at the
/// package directory's (pnpm/pnpm#13912).
#[test]
#[cfg_attr(target_os = "windows", ignore = "preferSymlinkedExecutables is inert on Windows")]
pub fn prefer_symlinked_executables_node_path_anchors_at_the_workspace_root() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\npreferSymlinkedExecutables: true\n",
    )
    .expect("write to pnpm-workspace.yaml");
    let pkg_dir = tmp.path().join("packages/app");
    fs::create_dir_all(&pkg_dir).expect("create workspace package dir");
    let config = Config::new().current::<HostNoHome>(&pkg_dir).expect("yaml is valid");
    assert_eq!(
        config.extra_env.get("NODE_PATH"),
        Some(&tmp.path().join("node_modules/.pnpm/node_modules").display().to_string()),
    );
}

/// The hoisted `nodeLinker` turns the setting on unless the user
/// configured it — but, like pnpm, the derived `true` exports no
/// `NODE_PATH`: pnpm computes `extraEnv` before its `nodeLinker`
/// switch, and the hoisted layout has no hidden store to expose.
#[test]
pub fn hoisted_node_linker_defaults_prefer_symlinked_executables_on() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "nodeLinker: hoisted\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert_eq!(config.prefer_symlinked_executables, Some(true));
    assert_eq!(config.extra_env.get("NODE_PATH"), None);

    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "nodeLinker: hoisted\npreferSymlinkedExecutables: false\n",
    )
    .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert_eq!(config.prefer_symlinked_executables, Some(false));
    assert_eq!(config.extra_env.get("NODE_PATH"), None);
}

/// The CLI's `--config.node-linker` override re-runs the derivation
/// after [`Config::current`], and it must track the *final* linker: a
/// `true` derived for the yaml-selected hoisted linker is dropped when
/// the override selects another linker (pnpm merges CLI options before
/// its `nodeLinker` switch), while a user-configured value survives.
#[test]
pub fn rederiving_prefer_symlinked_executables_follows_a_node_linker_override() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "nodeLinker: hoisted\n")
        .expect("write to pnpm-workspace.yaml");
    let mut config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert_eq!(config.prefer_symlinked_executables, Some(true));
    config.node_linker = crate::NodeLinker::Isolated;
    config.apply_prefer_symlinked_executables_derivation();
    assert_eq!(config.prefer_symlinked_executables, None);

    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "nodeLinker: hoisted\npreferSymlinkedExecutables: true\n",
    )
    .expect("write to pnpm-workspace.yaml");
    let mut config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    config.node_linker = crate::NodeLinker::Isolated;
    config.apply_prefer_symlinked_executables_derivation();
    assert_eq!(config.prefer_symlinked_executables, Some(true));
}

/// pnpm fails the process on an invalid `pnpm-workspace.yaml`.
/// `Config::current` must do the same instead of silently falling
/// back to defaults.
#[test]
pub fn invalid_workspace_yaml_propagates_error() {
    let tmp = tempdir().unwrap();
    // `: : :` is rejected by saphyr.
    fs::write(tmp.path().join("pnpm-workspace.yaml"), ": : :\n")
        .expect("write to pnpm-workspace.yaml");
    let result = Config::new().current::<HostNoHome>(tmp.path());
    let err = result.expect_err("invalid yaml should error");
    assert!(
        matches!(err, crate::LoadWorkspaceYamlError::ParseYaml { .. }),
        "expected ParseYaml, got {err:?}",
    );
}

/// Running `pacquet install` from a workspace subdirectory must
/// not leave `modules_dir` / `virtual_store_dir` anchored at the
/// CLI `--dir`. The presence of `pnpm-workspace.yaml` in an
/// ancestor signals that the workspace root is the install anchor,
/// matching pnpm v11, which anchors the install at the lockfile
/// directory. Without this, the per-importer `node_modules` writes
/// (under the
/// workspace root) and the virtual store (under the subdir) would
/// produce two inconsistent layouts for the same install.
#[test]
pub fn workspace_subdir_anchors_modules_at_workspace_root() {
    let tmp = tempdir().unwrap();
    let workspace_root = tmp.path();
    let subdir = workspace_root.join("packages/web");
    fs::create_dir_all(&subdir).expect("create subdir");
    fs::write(workspace_root.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write to pnpm-workspace.yaml");

    let config = Config::new().current::<HostNoHome>(&subdir).expect("config loads");

    assert_eq!(
        config.modules_dir,
        workspace_root.join("node_modules"),
        "modules_dir must be anchored at the workspace root, not the subdir",
    );
    assert_eq!(
        config.virtual_store_dir,
        workspace_root.join("node_modules/.pnpm"),
        "virtual_store_dir must be anchored at the workspace root, not the subdir",
    );
}

/// `NPM_CONFIG_WORKSPACE_DIR` must steer `Config::current`'s
/// path-anchoring just like it steers
/// [`pnpm_workspace::find_workspace_dir`] — otherwise the
/// virtual store would land in the cwd while the per-importer
/// `SymlinkDirectDependencies` writes land under the env-var
/// path, producing two `node_modules` layouts for the same
/// install. See PR [#443](https://github.com/pnpm/pacquet/pull/443).
///
/// Exercises the [`EnvVarOs`] DI seam: a per-test fake returns the
/// `env_workspace` path for the `NPM_CONFIG_WORKSPACE_DIR` lookup.
/// No `EnvGuard`, no `unsafe { env::set_var(...) }`.
#[test]
pub fn npm_config_workspace_dir_re_anchors_modules() {
    let env_workspace = tempdir().unwrap();
    let cwd_dir = tempdir().unwrap();
    static ENV_WORKSPACE_PATH: std::sync::OnceLock<OsString> = std::sync::OnceLock::new();
    ENV_WORKSPACE_PATH.set(env_workspace.path().as_os_str().to_owned()).expect("set once");
    struct HostWithEnvWorkspaceDir;
    impl EnvVar for HostWithEnvWorkspaceDir {
        fn var(name: &str) -> Option<String> {
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithEnvWorkspaceDir {
        fn var_os(name: &str) -> Option<OsString> {
            (name == "NPM_CONFIG_WORKSPACE_DIR")
                .then(|| ENV_WORKSPACE_PATH.get().expect("ENV_WORKSPACE_PATH initialised").clone())
        }
    }
    impl GetHomeDir for HostWithEnvWorkspaceDir {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithEnvWorkspaceDir);
    host_current_dir!(HostWithEnvWorkspaceDir);

    let config =
        Config::new().current::<HostWithEnvWorkspaceDir>(cwd_dir.path()).expect("config loads");
    assert_eq!(
        config.modules_dir,
        env_workspace.path().join("node_modules"),
        "modules_dir must follow NPM_CONFIG_WORKSPACE_DIR, not the cwd",
    );
    assert_eq!(
        config.virtual_store_dir,
        env_workspace.path().join("node_modules/.pnpm"),
        "virtual_store_dir must follow NPM_CONFIG_WORKSPACE_DIR, not the cwd",
    );
}

/// An empty `NPM_CONFIG_WORKSPACE_DIR` falls through to the
/// upward walk, matching pnpm, which treats only a non-empty
/// workspace-dir value as set. Pairs with `pnpm_workspace`'s
/// `empty_env_var_is_treated_as_unset`.
///
/// Drives the [`EnvVarOs`] DI seam with a fake that returns an
/// empty `OsString` for both spellings of the env var. The truthy
/// filter in `Config::current` should reject both, and the
/// install should fall through to the `start_dir`-walk.
#[test]
pub fn empty_npm_config_workspace_dir_falls_through() {
    struct HostWithEmptyEnvWorkspaceDir;
    impl EnvVar for HostWithEmptyEnvWorkspaceDir {
        fn var(name: &str) -> Option<String> {
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithEmptyEnvWorkspaceDir {
        fn var_os(name: &str) -> Option<OsString> {
            matches!(name, "NPM_CONFIG_WORKSPACE_DIR" | "npm_config_workspace_dir")
                .then(OsString::new)
        }
    }
    impl GetHomeDir for HostWithEmptyEnvWorkspaceDir {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithEmptyEnvWorkspaceDir);
    host_current_dir!(HostWithEmptyEnvWorkspaceDir);
    let tmp = tempdir().unwrap();
    let config =
        Config::new().current::<HostWithEmptyEnvWorkspaceDir>(tmp.path()).expect("config loads");
    // No yaml in tmp → no re-anchor → cwd-anchored defaults.
    assert_eq!(config.modules_dir, tmp.path().join("node_modules"));
    assert_eq!(config.virtual_store_dir, tmp.path().join("node_modules/.pnpm"));
}

#[test]
pub fn workspace_script_shell_accepts_backslash_path_like_values() {
    let workspace = tempdir().expect("workspace tempdir");
    fs::write(workspace.path().join("pnpm-workspace.yaml"), "scriptShell: 'scripts\\shell.cmd'\n")
        .expect("write workspace yaml");

    let config = Config::new().current::<HostNoHome>(workspace.path()).expect("config loads");
    let expected = pnpm_fs::lexical_normalize(&workspace.path().join(r"scripts\shell.cmd"))
        .to_string_lossy()
        .into_owned();
    assert_eq!(config.script_shell.as_deref(), Some(expected.as_str()));
}

#[cfg_attr(not(windows), ignore = "Windows path semantics")]
#[test]
pub fn workspace_script_shell_preserves_windows_absolute_and_unc_paths() {
    let workspace = tempdir().expect("workspace tempdir");
    for script_shell in
        [r"C:\tools\bash.exe", r"\\server\share\bash.exe", r"\tools\bash.exe", r"/tools/bash.exe"]
    {
        fs::write(
            workspace.path().join("pnpm-workspace.yaml"),
            format!("scriptShell: '{script_shell}'\n"),
        )
        .expect("write workspace yaml");

        let config = Config::new().current::<HostNoHome>(workspace.path()).expect("config loads");
        assert_eq!(config.script_shell.as_deref(), Some(script_shell));
    }
}

#[test]
pub fn pnpm_workspace_yaml_overrides_global_config_yaml() {
    let xdg = tempdir().unwrap();
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("config.yaml"), "enableGlobalVirtualStore: true\n")
        .expect("write to global config.yaml");

    let project = tempdir().unwrap();
    fs::write(project.path().join("pnpm-workspace.yaml"), "enableGlobalVirtualStore: false\n")
        .expect("write to pnpm-workspace.yaml");

    static XDG_CONFIG_HOME_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    XDG_CONFIG_HOME_PATH.set(xdg.path().to_path_buf()).expect("set once");

    struct HostWithXdgConfigHome;
    impl EnvVar for HostWithXdgConfigHome {
        fn var(name: &str) -> Option<String> {
            if name == "XDG_CONFIG_HOME" {
                return XDG_CONFIG_HOME_PATH.get().map(|path| path.to_string_lossy().into_owned());
            }
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithXdgConfigHome {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithXdgConfigHome {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithXdgConfigHome);
    host_current_dir!(HostWithXdgConfigHome);

    let config =
        Config::new().current::<HostWithXdgConfigHome>(project.path()).expect("config loads");
    assert!(
        !config.enable_global_virtual_store,
        "pnpm-workspace.yaml must win over global config.yaml",
    );
}

/// `virtualStoreDir` set in the global `config.yaml` is preserved
/// even when the workspace yaml doesn't repeat it. Without the
/// `!virtual_store_dir_explicit` guard on the re-anchor, the
/// workspace-root default (`<workspace>/node_modules/.pnpm`)
/// would overwrite the global value any time a `pnpm-workspace.yaml`
/// is present. Regression test for a `CodeRabbit` review finding on
/// pnpm/pnpm#11752.
#[test]
pub fn global_virtual_store_dir_survives_workspace_yaml_anchor() {
    let xdg = tempdir().unwrap();
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).unwrap();
    let global_path = xdg.path().join("shared-virtual-store");
    fs::write(
        config_dir.join("config.yaml"),
        format!("enableGlobalVirtualStore: true\nvirtualStoreDir: {}\n", global_path.display()),
    )
    .expect("write global config.yaml");

    let project = tempdir().unwrap();
    // Empty workspace yaml — present so the workspace block fires,
    // but it doesn't redeclare `virtualStoreDir`.
    fs::write(project.path().join("pnpm-workspace.yaml"), "packages:\n  - .\n")
        .expect("write workspace yaml");

    static XDG_CONFIG_HOME_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    XDG_CONFIG_HOME_PATH.set(xdg.path().to_path_buf()).expect("set once");

    struct HostWithXdgConfigHome;
    impl EnvVar for HostWithXdgConfigHome {
        fn var(name: &str) -> Option<String> {
            if name == "XDG_CONFIG_HOME" {
                return XDG_CONFIG_HOME_PATH.get().map(|path| path.to_string_lossy().into_owned());
            }
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithXdgConfigHome {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithXdgConfigHome {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithXdgConfigHome);
    host_current_dir!(HostWithXdgConfigHome);

    let config =
        Config::new().current::<HostWithXdgConfigHome>(project.path()).expect("config loads");
    assert_eq!(
        config.virtual_store_dir, global_path,
        "virtualStoreDir from global config.yaml must survive the workspace-root re-anchor",
    );
}

/// Workspace-only keys in the global `config.yaml` are silently
/// ignored, matching pnpm. A `nodeLinker: hoisted` in the global
/// yaml would change the installer's layout strategy if applied —
/// pnpm rejects it, and pacquet must too.
#[test]
pub fn global_config_yaml_workspace_only_keys_are_ignored() {
    let xdg = tempdir().unwrap();
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("config.yaml"),
        // `nodeLinker`, `hoist`, `symlink`, and `lockfile` are all
        // workspace-only keys pnpm excludes from the global config.
        // None should apply when set in the global config.
        "nodeLinker: hoisted\nhoist: false\nsymlink: false\nlockfile: false\n",
    )
    .expect("write to global config.yaml");

    static XDG_CONFIG_HOME_PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    XDG_CONFIG_HOME_PATH.set(xdg.path().to_path_buf()).expect("set once");

    struct HostWithXdgConfigHome;
    impl EnvVar for HostWithXdgConfigHome {
        fn var(name: &str) -> Option<String> {
            if name == "XDG_CONFIG_HOME" {
                return XDG_CONFIG_HOME_PATH.get().map(|path| path.to_string_lossy().into_owned());
            }
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithXdgConfigHome {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithXdgConfigHome {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithXdgConfigHome);
    host_current_dir!(HostWithXdgConfigHome);

    let tmp = tempdir().unwrap();
    let defaults = Config::new();
    let config = Config::new().current::<HostWithXdgConfigHome>(tmp.path()).expect("config loads");
    assert_eq!(config.node_linker, defaults.node_linker);
    assert_eq!(config.hoist, defaults.hoist);
    assert_eq!(config.symlink, defaults.symlink);
    assert_eq!(config.lockfile, defaults.lockfile);
}

/// `PNPM_CONFIG_*` overrides `pnpm-workspace.yaml` — the env
/// var is applied after yaml in pnpm's reader cascade. Without
/// this ordering a CI override via env var couldn't beat a
/// committed yaml setting, which is the whole reason to expose
/// env vars at all.
#[test]
pub fn pnpm_config_env_var_overrides_workspace_yaml() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "enableGlobalVirtualStore: false\n")
        .expect("write to pnpm-workspace.yaml");

    struct HostWithPnpmConfigEnv;
    impl EnvVar for HostWithPnpmConfigEnv {
        fn var(name: &str) -> Option<String> {
            if name == "PNPM_CONFIG_ENABLE_GLOBAL_VIRTUAL_STORE" {
                return Some("true".to_owned());
            }
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithPnpmConfigEnv {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithPnpmConfigEnv {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithPnpmConfigEnv);
    host_current_dir!(HostWithPnpmConfigEnv);

    let config = Config::new().current::<HostWithPnpmConfigEnv>(tmp.path()).expect("loads");
    assert!(
        config.enable_global_virtual_store,
        "PNPM_CONFIG_* env var must win over pnpm-workspace.yaml",
    );
}
