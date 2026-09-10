use super::{
    Config, EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, HostNoHome, LinkProbe,
    LoadWorkspaceYamlError, OsString, Path, PathBuf, STORED_LOGIN, assert_eq, capture_warnings, fs,
    io, load_with_auth_file, load_with_project_and_user, tempdir, write_file,
    write_registry_auth_file,
};

#[test]
pub fn npmrc_auth_file_override_supplies_auth() {
    let project = tempdir().expect("project tempdir");
    let auth = tempdir().expect("auth tempdir");
    let auth_file = auth.path().join("custom-npmrc");
    fs::write(
        &auth_file,
        "registry=https://registry.example.com/\n\
         //registry.example.com/:_authToken=secret-token\n",
    )
    .expect("write auth file");

    let config = Config { npmrc_auth_file: Some(auth_file), ..Config::default() }
        .current::<HostNoHome>(project.path())
        .expect("load config");

    assert_eq!(config.registry, "https://registry.example.com/");
    assert_eq!(
        config.auth_headers.for_url("https://registry.example.com/some-pkg").as_deref(),
        Some("Bearer secret-token"),
    );
}

/// The `.npmrc` an `npmrcAuthFile` points at is trusted, so its `_auth`
/// authenticates the package-manager bootstrap too — the path a
/// `devEngines` pin resolves `@pnpm/exe` through (pnpm/pnpm#14257).
#[test]
pub fn npmrc_auth_file_override_supplies_basic_auth_to_bootstrap() {
    let project = tempdir().expect("project tempdir");
    let auth = tempdir().expect("auth tempdir");
    let auth_file = auth.path().join("custom-npmrc");
    let pair = pnpm_network::base64_encode("alice:p@ss");
    fs::write(
        &auth_file,
        format!(
            "registry=https://registry.example.com/\n\
             //registry.example.com/:_auth={pair}\n",
        ),
    )
    .expect("write auth file");

    let config = Config { npmrc_auth_file: Some(auth_file), ..Config::default() }
        .current::<HostNoHome>(project.path())
        .expect("load config");

    assert_eq!(
        config
            .package_manager_bootstrap
            .auth_headers
            .for_url_with_package("https://registry.example.com/@pnpm%2Fexe", Some("@pnpm/exe"))
            .as_deref(),
        Some(format!("Basic {pair}").as_str()),
    );
}

#[test]
pub fn npmrc_auth_file_from_pnpm_config_env() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    let auth = tempdir().expect("auth tempdir");
    let auth_file = auth.path().join("ci-npmrc");
    write_registry_auth_file(&auth_file, "https://ci.example.com/", "ci-token");

    set_fake_env(&[("PNPM_CONFIG_NPMRC_AUTH_FILE", auth_file.to_str().unwrap())]);
    let config = load_with_fake_env(project.path());

    assert_eq!(config.registry, "https://ci.example.com/");
    assert_eq!(
        config.auth_headers.for_url("https://ci.example.com/pkg").as_deref(),
        Some("Bearer ci-token"),
    );
}

#[test]
pub fn npmrc_auth_file_from_lowercase_pnpm_config_env() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    let auth = tempdir().expect("auth tempdir");
    let auth_file = auth.path().join("ci-npmrc");
    write_registry_auth_file(&auth_file, "https://ci.example.com/", "ci-token");

    set_fake_env(&[("pnpm_config_npmrc_auth_file", auth_file.to_str().unwrap())]);
    let config = load_with_fake_env(project.path());

    assert_eq!(
        config.auth_headers.for_url("https://ci.example.com/pkg").as_deref(),
        Some("Bearer ci-token"),
    );
}

#[test]
pub fn npmrc_auth_file_empty_env_falls_through_to_userconfig() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    let auth = tempdir().expect("auth tempdir");
    let auth_file = auth.path().join("user-npmrc");
    write_registry_auth_file(&auth_file, "https://user.example.com/", "user-token");

    set_fake_env(&[
        ("PNPM_CONFIG_NPMRC_AUTH_FILE", ""),
        ("PNPM_CONFIG_USERCONFIG", auth_file.to_str().unwrap()),
    ]);
    let config = load_with_fake_env(project.path());

    assert_eq!(
        config.auth_headers.for_url("https://user.example.com/pkg").as_deref(),
        Some("Bearer user-token"),
    );
}

#[test]
pub fn npmrc_auth_file_outranks_userconfig() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    let auth = tempdir().expect("auth tempdir");
    let auth_file = auth.path().join("auth-file");
    let userconfig = auth.path().join("userconfig");
    write_registry_auth_file(&auth_file, "https://authfile.example.com/", "authfile-token");
    write_registry_auth_file(&userconfig, "https://userconfig.example.com/", "userconfig-token");

    set_fake_env(&[
        ("PNPM_CONFIG_NPMRC_AUTH_FILE", auth_file.to_str().unwrap()),
        ("PNPM_CONFIG_USERCONFIG", userconfig.to_str().unwrap()),
    ]);
    let config = load_with_fake_env(project.path());

    assert_eq!(config.registry, "https://authfile.example.com/");
    assert_eq!(
        config.auth_headers.for_url("https://authfile.example.com/pkg").as_deref(),
        Some("Bearer authfile-token"),
    );
}

#[test]
pub fn npmrc_auth_file_npm_config_userconfig_is_compat_fallback() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    let auth = tempdir().expect("auth tempdir");
    let npm_file = auth.path().join("npm-userconfig");
    write_registry_auth_file(&npm_file, "https://npm.example.com/", "npm-token");

    set_fake_env(&[("npm_config_userconfig", npm_file.to_str().unwrap())]);
    let config = load_with_fake_env(project.path());
    assert_eq!(
        config.auth_headers.for_url("https://npm.example.com/pkg").as_deref(),
        Some("Bearer npm-token"),
    );

    let pnpm_file = auth.path().join("pnpm-userconfig");
    write_registry_auth_file(&pnpm_file, "https://pnpm.example.com/", "pnpm-token");
    set_fake_env(&[
        ("PNPM_CONFIG_USERCONFIG", pnpm_file.to_str().unwrap()),
        ("npm_config_userconfig", npm_file.to_str().unwrap()),
    ]);
    let config = load_with_fake_env(project.path());
    assert_eq!(
        config.auth_headers.for_url("https://pnpm.example.com/pkg").as_deref(),
        Some("Bearer pnpm-token"),
    );
}

#[test]
pub fn global_config_npmrc_auth_file_expands_env() {
    fake_env!(load_with_fake_env);
    let xdg = tempdir().expect("xdg tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create config dir");

    let auth = tempdir().expect("auth tempdir");
    let auth_file = auth.path().join("global-npmrc");
    write_registry_auth_file(&auth_file, "https://global-auth.example.com/", "global-token");
    fs::write(config_dir.join("config.yaml"), "npmrcAuthFile: ${AUTH_FILE}\n")
        .expect("write global config.yaml");

    let project = tempdir().expect("project tempdir");
    set_fake_env(&[
        ("AUTH_FILE", auth_file.to_str().unwrap()),
        ("XDG_CONFIG_HOME", xdg.path().to_str().unwrap()),
    ]);
    let config = load_with_fake_env(project.path());

    assert_eq!(
        config.auth_headers.for_url("https://global-auth.example.com/pkg").as_deref(),
        Some("Bearer global-token"),
    );
}

/// An unscoped `_authToken` in the user-level file pins to *that
/// file's* registry, never the workspace registry — even when the
/// project `.npmrc` overrides the default registry to something else.
/// This is the credential-isolation boundary ported from pnpm.
#[test]
pub fn user_auth_token_pins_to_its_own_file_registry() {
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    write_file(&user_file, "registry=https://trusted.example.com/\n_authToken=user-secret\n");

    let config = load_with_project_and_user("registry=https://attacker.example.com/\n", user_file);

    assert_eq!(config.registry, "https://attacker.example.com/", "project registry wins");
    assert_eq!(
        config.auth_headers.for_url("https://trusted.example.com/pkg").as_deref(),
        Some("Bearer user-secret"),
    );
    assert_eq!(config.auth_headers.for_url("https://attacker.example.com/pkg"), None);
}

#[test]
pub fn url_scoped_env_auth_is_used_and_outranks_project_npmrc() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(&project.path().join(".npmrc"), "//env2e.example.com/:_authToken=project-token\n");
    set_fake_env(&[("npm_config_//env2e.example.com/:_authToken", "env-token")]);

    let config = load_with_fake_env(project.path());

    assert_eq!(
        config.auth_headers.for_url("https://env2e.example.com/pkg").as_deref(),
        Some("Bearer env-token"),
    );
}

#[test]
pub fn url_scoped_env_auth_prefix_is_case_insensitive_end_to_end() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    set_fake_env(&[("NPM_CONFIG_//env2e.example.com/:_authToken", "upper-token")]);

    let config = load_with_fake_env(project.path());

    assert_eq!(
        config.auth_headers.for_url("https://env2e.example.com/pkg").as_deref(),
        Some("Bearer upper-token"),
    );
}

#[test]
pub fn json_env_host_keyed_token_is_used_and_outranks_project_npmrc() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(&project.path().join(".npmrc"), "//json2e.example.com/:_authToken=project-token\n");
    set_fake_env(&[(
        "pnpm_config__auth",
        r#"{"https://json2e.example.com":{"@":{"authToken":"env-token"}}}"#,
    )]);

    let config = load_with_fake_env(project.path());

    assert_eq!(
        config.auth_headers.for_url("https://json2e.example.com/pkg").as_deref(),
        Some("Bearer env-token"),
    );
}

#[test]
pub fn json_env_repo_registry_cannot_redirect_token() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join("pnpm-workspace.yaml"),
        "registries:\n  '@org-a': https://attacker.example/\n",
    );
    set_fake_env(&[(
        "pnpm_config__auth",
        r#"{"https://npm.pkg.github.com":{"@org-a":{"authToken":"org-a-token"}}}"#,
    )]);

    let config = load_with_fake_env(project.path());

    assert_eq!(
        config
            .auth_headers
            .for_url_with_package("https://npm.pkg.github.com/org-a/foo", Some("@org-a/foo"))
            .as_deref(),
        Some("Bearer org-a-token"),
    );
    // Use the scoped lookup the token is keyed under — `for_url` alone
    // only checks the default/unscoped path and would pass even if the
    // `@org-a` token had been rebound to the attacker host.
    assert!(
        config
            .auth_headers
            .for_url_with_package("https://attacker.example/org-a/foo", Some("@org-a/foo"))
            .is_none(),
        "repo-controlled registry URL must not receive the env token",
    );
}

#[test]
pub fn json_env_per_scope_token_on_shared_host() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    set_fake_env(&[(
        "pnpm_config__auth",
        r#"{"https://npm.pkg.github.com":{"@org-a":{"authToken":"a-tok"},"@org-b":{"authToken":"b-tok"}}}"#,
    )]);

    let config = load_with_fake_env(project.path());

    assert_eq!(
        config
            .auth_headers
            .for_url_with_package("https://npm.pkg.github.com/org-a/foo", Some("@org-a/foo"))
            .as_deref(),
        Some("Bearer a-tok"),
    );
    assert_eq!(
        config
            .auth_headers
            .for_url_with_package("https://npm.pkg.github.com/org-b/foo", Some("@org-b/foo"))
            .as_deref(),
        Some("Bearer b-tok"),
    );
}

/// End-to-end: a `registries` alias in the user's global `config.yaml`
/// cannot rebind a `pnpm_config__auth` token to a different host. The
/// `_auth` routes sit above global-config registries in the merge, so the
/// `@victim-scope` token stays on its declared host and the attacker host
/// receives no credential. Mirrors the reader test
/// `global config.yaml registries cannot redirect pnpm_config__auth routes`.
#[test]
pub fn global_config_yaml_registries_cannot_redirect_json_env_token() {
    fake_env!(load_with_fake_env);
    let xdg = tempdir().expect("xdg tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(
        config_dir.join("config.yaml"),
        "registries:\n  '@victim-scope': https://attacker.example/\n",
    )
    .expect("write global config.yaml");

    let project = tempdir().expect("project tempdir");
    set_fake_env(&[
        ("XDG_CONFIG_HOME", xdg.path().to_str().unwrap()),
        (
            "pnpm_config__auth",
            r#"{"https://npm.pkg.github.com":{"@victim-scope":{"authToken":"secret-token"}}}"#,
        ),
    ]);
    let config = load_with_fake_env(project.path());

    assert_eq!(
        config.registries_by_scope.get("@victim-scope").map(String::as_str),
        Some("https://npm.pkg.github.com/"),
    );
    assert_eq!(
        config
            .auth_headers
            .for_url_with_package(
                "https://npm.pkg.github.com/victim-scope/foo",
                Some("@victim-scope/foo")
            )
            .as_deref(),
        Some("Bearer secret-token"),
    );
    assert!(
        config
            .auth_headers
            .for_url_with_package(
                "https://attacker.example/victim-scope/foo",
                Some("@victim-scope/foo")
            )
            .is_none(),
        "global-config registry alias must not receive the env token",
    );
}

/// End-to-end: the `_auth` key of the global pnpm `config.yaml`
/// configures registry auth and the inferred routes, just like the
/// `pnpm_config__auth` env var.
#[test]
pub fn global_config_yaml_auth_configures_registry_auth() {
    fake_env!(load_with_fake_env);
    let xdg = tempdir().expect("xdg tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(
        config_dir.join("config.yaml"),
        "_auth:\n  \"https://global-auth.example.com\":\n    \"@\":\n      authToken: yaml-token\n    \"@org\":\n      authToken: org-yaml-token\n",
    )
    .expect("write global config.yaml");

    let project = tempdir().expect("project tempdir");
    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);
    let config = load_with_fake_env(project.path());

    assert_eq!(
        config.auth_headers.for_url("https://global-auth.example.com/pkg").as_deref(),
        Some("Bearer yaml-token"),
    );
    assert_eq!(config.registry, "https://global-auth.example.com/");
    assert_eq!(
        config.registries_by_scope.get("@org").map(String::as_str),
        Some("https://global-auth.example.com/"),
    );
}

/// End-to-end: the `pnpm_config__auth` env var wins over the global
/// `config.yaml` `_auth` on a conflicting key.
#[test]
pub fn json_env_auth_wins_over_global_config_yaml_auth() {
    fake_env!(load_with_fake_env);
    let xdg = tempdir().expect("xdg tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(
        config_dir.join("config.yaml"),
        "_auth:\n  \"https://shared.example.com\":\n    \"@\":\n      authToken: yaml-token\n",
    )
    .expect("write global config.yaml");

    let project = tempdir().expect("project tempdir");
    set_fake_env(&[
        ("XDG_CONFIG_HOME", xdg.path().to_str().unwrap()),
        ("pnpm_config__auth", r#"{"https://shared.example.com":{"@":{"authToken":"env-token"}}}"#),
    ]);
    let config = load_with_fake_env(project.path());

    assert_eq!(
        config.auth_headers.for_url("https://shared.example.com/pkg").as_deref(),
        Some("Bearer env-token"),
    );
}

/// End-to-end: `_auth` in a project `pnpm-workspace.yaml` is ignored —
/// repo-controlled config must never supply registry credentials.
#[test]
pub fn project_workspace_yaml_auth_is_ignored() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join("pnpm-workspace.yaml"),
        "_auth:\n  \"https://attacker.example\":\n    \"@\":\n      authToken: attacker-token\n",
    );
    set_fake_env(&[]);

    let config = load_with_fake_env(project.path());

    assert!(
        config.auth_headers.for_url("https://attacker.example/pkg").is_none(),
        "project pnpm-workspace.yaml _auth must not configure registry auth",
    );
    assert_ne!(config.registry, "https://attacker.example/");
}

#[test]
pub fn json_env_invalid_auth_aborts_the_load() {
    fake_env!();
    // An unsupported field and a non-string token are both hard errors, so
    // no partially-applied routing leaks through.
    let project = tempdir().expect("project tempdir");
    set_fake_env(&[(
        "pnpm_config__auth",
        r#"{"https://private.example":{"@":{"tokenAuth":"tok"},"@org":{"authToken":123}}}"#,
    )]);

    let result = Config::default().current::<FakeEnv>(project.path());
    assert!(matches!(result, Err(LoadWorkspaceYamlError::InvalidJsonAuth { .. })));
}

#[test]
pub fn user_basic_auth_pins_to_its_own_file_registry() {
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    // base64("user:pass")
    write_file(&user_file, "registry=https://trusted.example.com/\n_auth=dXNlcjpwYXNz\n");

    let config = load_with_project_and_user("registry=https://attacker.example.com/\n", user_file);

    assert_eq!(
        config.auth_headers.for_url("https://trusted.example.com/pkg").as_deref(),
        Some("Basic dXNlcjpwYXNz"),
    );
    assert_eq!(config.auth_headers.for_url("https://attacker.example.com/pkg"), None);
}

#[test]
pub fn workspace_npmrc_overrides_global_auth_file() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    fs::write(project.path().join(".npmrc"), "//registry.npmjs.org/:_authToken=workspace-token\n")
        .expect("write workspace .npmrc");

    let xdg = tempdir().expect("config tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create global config dir");
    fs::write(config_dir.join("auth.ini"), "//registry.npmjs.org/:_authToken=global-token\n")
        .expect("write global auth file");

    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);
    let config = load_with_fake_env(project.path());

    assert_eq!(
        config.auth_headers.for_url("https://registry.npmjs.org/pkg").as_deref(),
        Some("Bearer workspace-token"),
    );
}

/// Unscoped inline `cert`/`key` pin to the file's registry as
/// per-registry TLS, never to the workspace registry or the global
/// client identity.
#[test]
pub fn user_cert_key_pin_to_its_own_file_registry() {
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    write_file(&user_file, "registry=https://trusted.example.com/\ncert=cert-pem\nkey=key-pem\n");

    let config = load_with_project_and_user("registry=https://attacker.example.com/\n", user_file);

    assert_eq!(config.tls.cert, None, "cert is rescoped, not a global identity");
    assert_eq!(config.tls.key, None);
    let scoped =
        config.tls_by_uri.get("//trusted.example.com/").expect("cert/key pinned to trusted");
    assert_eq!(scoped.cert.as_deref(), Some("cert-pem"));
    assert_eq!(scoped.key.as_deref(), Some("key-pem"));
    assert!(config.tls_by_uri.get("//attacker.example.com/").is_none());
}

/// A registry override that lands *after* the `.npmrc` files are read —
/// here `PNPM_CONFIG_REGISTRY`, the in-cascade twin of `--registry` —
/// does not pull an ambient user-level token along with it. The token
/// pinned to the npmjs default when the user file was read.
#[test]
pub fn late_registry_override_does_not_pull_an_unscoped_user_token_along() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    write_file(&user_file, "_authToken=user-secret\n");

    set_fake_env(&[
        ("PNPM_CONFIG_NPMRC_AUTH_FILE", user_file.to_str().unwrap()),
        ("PNPM_CONFIG_REGISTRY", "https://attacker.example.com/"),
    ]);
    let config = load_with_fake_env(project.path());

    assert_eq!(config.registry, "https://attacker.example.com/");
    assert_eq!(
        config.auth_headers.for_url("https://registry.npmjs.org/pkg").as_deref(),
        Some("Bearer user-secret"),
    );
    assert_eq!(config.auth_headers.for_url("https://attacker.example.com/pkg"), None);
}

/// An unscoped `tokenHelper` is honored but has no INI-readable spelling
/// of its own, so the parser never captures it verbatim. It is still
/// reported under the key it was pinned to, matching a helper written
/// URL-scoped by hand.
#[test]
pub fn rescoped_token_helper_is_reported_under_its_pinned_key() {
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    write_file(&user_file, "registry=https://trusted.example.com/\ntokenHelper=/bin/echo\n");

    let config = load_with_project_and_user("", user_file);

    assert_eq!(
        config.raw_auth_config.get("//trusted.example.com/:tokenHelper").map(String::as_str),
        Some("/bin/echo"),
    );
}

/// `auth.ini` (in the global config dir) with no `registry=` of its
/// own falls back to the npmjs default for its unscoped creds — it
/// does not borrow the user file's or workspace's registry.
#[test]
pub fn auth_ini_without_registry_falls_back_to_npmjs_default() {
    fake_env!();
    let project = tempdir().expect("project tempdir");
    write_file(&project.path().join(".npmrc"), "registry=https://attacker.example.com/\n");
    let config_home = tempdir().expect("config-home tempdir");
    let pnpm_dir = config_home.path().join("pnpm");
    fs::create_dir_all(&pnpm_dir).expect("create pnpm config dir");
    write_file(&pnpm_dir.join("auth.ini"), "_authToken=auth-ini-secret\n");
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    write_file(&user_file, "registry=https://trusted.example.com/\n");

    set_fake_env(&[("XDG_CONFIG_HOME", config_home.path().to_str().unwrap())]);
    let config = Config { npmrc_auth_file: Some(user_file), ..Config::default() }
        .current::<FakeEnv>(project.path())
        .expect("load config");

    assert_eq!(
        config.auth_headers.for_url("https://registry.npmjs.org/pkg").as_deref(),
        Some("Bearer auth-ini-secret"),
    );
    assert_eq!(config.auth_headers.for_url("https://attacker.example.com/pkg"), None);
    assert_eq!(config.auth_headers.for_url("https://trusted.example.com/pkg"), None);
}

/// A `tokenHelper` set in the global pnpm `auth.ini` (a trusted, non-repo
/// source) is honored: its command runs lazily on lookup and its stdout
/// becomes the `Authorization` header. `/bin/echo` is a real binary, so
/// this exercises the whole path end to end (Unix only).
#[cfg(unix)]
#[test]
pub fn token_helper_in_global_auth_ini_is_honored() {
    fake_env!();
    let project = tempdir().expect("project tempdir");
    let config_home = tempdir().expect("config-home tempdir");
    let pnpm_dir = config_home.path().join("pnpm");
    fs::create_dir_all(&pnpm_dir).expect("create pnpm config dir");
    write_file(
        &pnpm_dir.join("auth.ini"),
        "//registry.example.com/:tokenHelper=/bin/echo s3cr3t\n",
    );

    set_fake_env(&[("XDG_CONFIG_HOME", config_home.path().to_str().unwrap())]);
    let config =
        Config::default().current::<FakeEnv>(project.path()).expect("load config with tokenHelper");

    assert_eq!(
        config.auth_headers.for_url("https://registry.example.com/pkg").as_deref(),
        Some("Bearer s3cr3t"),
    );
}

/// A `tokenHelper` in a project `.npmrc` is rejected: a checked-in
/// `.npmrc` must not be able to run an arbitrary command.
#[test]
pub fn token_helper_in_project_npmrc_is_rejected() {
    fake_env!();
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join(".npmrc"),
        "//registry.example.com/:tokenHelper=/bin/echo s3cr3t\n",
    );

    set_fake_env(&[]);
    let error = Config::default()
        .current::<FakeEnv>(project.path())
        .expect_err("project tokenHelper must be rejected");
    assert!(
        matches!(error, LoadWorkspaceYamlError::TokenHelperInProjectConfig { .. }),
        "got {error:?}",
    );
}

/// A trusted `tokenHelper` carrying a reserved character (here `$`, which
/// pnpm reserves for future interpolation) is rejected at config load.
#[test]
pub fn token_helper_with_reserved_character_is_rejected() {
    fake_env!();
    let project = tempdir().expect("project tempdir");
    let config_home = tempdir().expect("config-home tempdir");
    let pnpm_dir = config_home.path().join("pnpm");
    fs::create_dir_all(&pnpm_dir).expect("create pnpm config dir");
    write_file(&pnpm_dir.join("auth.ini"), "//registry.example.com/:tokenHelper=echo $SECRET\n");

    set_fake_env(&[("XDG_CONFIG_HOME", config_home.path().to_str().unwrap())]);
    let error = Config::default()
        .current::<FakeEnv>(project.path())
        .expect_err("reserved character must be rejected");
    assert!(
        matches!(error, LoadWorkspaceYamlError::TokenHelperUnsupportedCharacter { character: '$' }),
        "got {error:?}",
    );
}

/// A `tokenHelper` supplied through a URL-scoped environment variable is
/// dropped, not honored: the environment layer must never run an arbitrary
/// command. Mirrors pnpm dropping `//host/:tokenHelper` env vars.
#[test]
pub fn token_helper_from_url_scoped_env_is_not_honored() {
    fake_env!();
    let project = tempdir().expect("project tempdir");

    set_fake_env(&[("npm_config_//registry.example.com/:tokenHelper", "/bin/echo s3cr3t")]);
    let config = Config::default()
        .current::<FakeEnv>(project.path())
        .expect("env tokenHelper is dropped, not an error");

    assert_eq!(config.auth_headers.for_url("https://registry.example.com/pkg"), None);
}

#[test]
pub fn non_auth_keys_in_npmrc_are_ignored() {
    // pnpm 11 stopped reading project-structural settings from .npmrc.
    // Writing `symlink=false` / `lockfile=true` / hoist / node-linker /
    // store-dir to .npmrc should have no effect on the resolved config.
    let tmp = tempdir().unwrap();
    let non_auth_ini = "symlink=false\nlockfile=true\nhoist=false\nnode-linker=hoisted\n";
    fs::write(tmp.path().join(".npmrc"), non_auth_ini).expect("write to .npmrc");
    let defaults = Config::new();
    let config =
        Config::new().current::<HostNoHome>(tmp.path()).expect("workspace yaml absent => no error");
    assert_eq!(config.symlink, defaults.symlink);
    assert_eq!(config.lockfile, defaults.lockfile);
    assert_eq!(config.hoist, defaults.hoist);
    assert_eq!(config.node_linker, defaults.node_linker);
}

// Regression test for pnpm/pnpm#12480: when PNPM_CONFIG_NPMRC_AUTH_FILE points
// at the project .npmrc, no "Ignored project-level auth setting" warning should fire.
#[test]
pub fn npmrc_auth_file_pointing_at_project_npmrc_suppresses_warning() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    let project_npmrc = project.path().join(".npmrc");
    fs::write(&project_npmrc, "//registry.npmjs.org/:_authToken=${MY_TOKEN}\n")
        .expect("write project .npmrc");

    set_fake_env(&[
        ("MY_TOKEN", "secret-token"),
        ("PNPM_CONFIG_NPMRC_AUTH_FILE", project_npmrc.to_str().unwrap()),
    ]);

    let warnings = capture_warnings(|| {
        load_with_fake_env(project.path());
    });

    let auth_warnings: Vec<_> =
        warnings.iter().filter(|w| w.contains("Ignored project-level auth setting")).collect();
    assert!(
        auth_warnings.is_empty(),
        "expected no auth warning when PNPM_CONFIG_NPMRC_AUTH_FILE points at project .npmrc, got: {auth_warnings:?}",
    );
}

// The exact shape reported in pnpm/pnpm#12480: a relative
// `PNPM_CONFIG_NPMRC_AUTH_FILE=.npmrc`, anchored at the cwd.
#[test]
pub fn npmrc_auth_file_relative_to_cwd_pointing_at_project_npmrc_suppresses_warning() {
    fake_env!(set_fake_cwd, load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    fs::write(project.path().join(".npmrc"), "//registry.npmjs.org/:_authToken=${MY_TOKEN}\n")
        .expect("write project .npmrc");

    set_fake_env(&[("MY_TOKEN", "secret-token"), ("PNPM_CONFIG_NPMRC_AUTH_FILE", ".npmrc")]);
    set_fake_cwd(project.path());

    let mut config = None;
    let warnings = capture_warnings(|| {
        config = Some(load_with_fake_env(project.path()));
    });

    let auth_warnings: Vec<_> =
        warnings.iter().filter(|w| w.contains("Ignored project-level auth setting")).collect();
    assert!(
        auth_warnings.is_empty(),
        "expected no auth warning for a relative npmrcAuthFile that resolves to the project .npmrc, got: {auth_warnings:?}",
    );
    assert_eq!(
        config.unwrap().auth_headers.for_url("https://registry.npmjs.org/pkg").as_deref(),
        Some("Bearer secret-token"),
        "the trusted project .npmrc must expand the auth env placeholder",
    );
}

// A relative npmrcAuthFile that resolves somewhere other than the project
// .npmrc must not trust it — the warning stays.
#[test]
pub fn npmrc_auth_file_relative_resolving_elsewhere_keeps_warning() {
    fake_env!(set_fake_cwd, load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    let elsewhere = tempdir().expect("elsewhere tempdir");
    fs::write(project.path().join(".npmrc"), "//registry.npmjs.org/:_authToken=${MY_TOKEN}\n")
        .expect("write project .npmrc");

    set_fake_env(&[("MY_TOKEN", "secret-token"), ("PNPM_CONFIG_NPMRC_AUTH_FILE", ".npmrc")]);
    set_fake_cwd(elsewhere.path());

    let warnings = capture_warnings(|| {
        load_with_fake_env(project.path());
    });

    assert!(
        warnings.iter().any(|w| w.contains("Ignored project-level auth setting")),
        "expected the auth warning when the relative npmrcAuthFile does not resolve to the project .npmrc, got: {warnings:?}",
    );
}

/// The global config file's `_auth` is where a `pnpm login` stores what it
/// was granted. Holding a credential for a registry is not a statement that
/// packages come from it, so a project that declares its own registry keeps
/// it.
#[test]
pub fn a_declared_registry_beats_the_global_auth_file() {
    let config =
        load_with_auth_file(STORED_LOGIN, Some("registry: https://project-choice.example/\n"));

    assert_eq!(config.registry, "https://project-choice.example/");
    // The credential still reaches the registry it was written for.
    assert_eq!(
        config.auth_tokens_by_uri.get("//private.example/").map(String::as_str),
        Some("stored-token"),
    );
}

#[test]
pub fn a_declared_scope_route_beats_the_global_auth_file() {
    let config = load_with_auth_file(
        STORED_LOGIN,
        Some("registries:\n  https://project-org.example/:\n    scopes: ['@org']\n"),
    );

    assert_eq!(
        config.registries_by_scope.get("@org").map(String::as_str),
        Some("https://project-org.example/"),
    );
}
