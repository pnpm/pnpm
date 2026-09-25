use super::{
    Config, EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, LinkProbe, OsString, Path, PathBuf,
    assert_eq, io, tempdir, write_file,
};

/// Regression test for pnpm/pnpm#15530:
/// When `.npmrc` sets `registry=https://nexus.abc.de/repository/npm-public/`,
/// `pnpm-workspace.yaml` configures scoped registries for `@abc` and `@pong`,
/// and `pnpm_config__auth` sets credentials for all three registries using `@`,
/// the default registry must be preserved as `npm-public/`, and must not be
/// overwritten by the last entry in `pnpm_config__auth`.
#[test]
pub fn json_env_preserves_default_registry_when_multiple_registries_and_scoped_registries_configured()
 {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join(".npmrc"),
        "registry=https://nexus.abc.de/repository/npm-public/\n",
    );
    write_file(
        &project.path().join("pnpm-workspace.yaml"),
        "registries:\n  'https://nexus.abc.de/repository/npm-hosted/':\n    scopes: ['@abc']\n  'https://nexus.abc.de/repository/mode2-npm-hosted/':\n    scopes: ['@pong']\n",
    );
    set_fake_env(&[(
        "pnpm_config__auth",
        r#"{
            "https://nexus.abc.de/repository/npm-public/": { "@": { "authToken": "token-public" } },
            "https://nexus.abc.de/repository/npm-hosted/": { "@": { "authToken": "token-abc" } },
            "https://nexus.abc.de/repository/mode2-npm-hosted/": { "@": { "authToken": "token-pong" } }
        }"#,
    )]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.registry, "https://nexus.abc.de/repository/npm-public/");
    assert_eq!(
        config.registries_by_scope.get("default").map(String::as_str),
        Some("https://nexus.abc.de/repository/npm-public/"),
    );
    assert_eq!(
        config.registries_by_scope.get("@abc").map(String::as_str),
        Some("https://nexus.abc.de/repository/npm-hosted/"),
    );
    assert_eq!(
        config.registries_by_scope.get("@pong").map(String::as_str),
        Some("https://nexus.abc.de/repository/mode2-npm-hosted/"),
    );

    let resolved = config.resolved_registries();
    assert_eq!(
        resolved.get("default").map(String::as_str),
        Some("https://nexus.abc.de/repository/npm-public/"),
    );
    assert_eq!(
        resolved.get("@abc").map(String::as_str),
        Some("https://nexus.abc.de/repository/npm-hosted/"),
    );
    assert_eq!(
        resolved.get("@pong").map(String::as_str),
        Some("https://nexus.abc.de/repository/mode2-npm-hosted/"),
    );

    assert_eq!(
        config.auth_headers.for_url("https://nexus.abc.de/repository/npm-public/pkg").as_deref(),
        Some("Bearer token-public"),
    );
    assert_eq!(
        config.auth_headers
            .for_url("https://nexus.abc.de/repository/npm-hosted/@abc/pkg")
            .as_deref(),
        Some("Bearer token-abc"),
    );
    assert_eq!(
        config.auth_headers
            .for_url("https://nexus.abc.de/repository/mode2-npm-hosted/@pong/pkg")
            .as_deref(),
        Some("Bearer token-pong"),
    );
}

/// A scoped registry credential in `pnpm_config__auth` must not overwrite
/// the default registry even when no default entry is in `pnpm_config__auth`.
#[test]
pub fn json_env_scoped_registries_only_do_not_overwrite_default_registry() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join(".npmrc"),
        "registry=https://nexus.abc.de/repository/npm-public/\n",
    );
    write_file(
        &project.path().join("pnpm-workspace.yaml"),
        "registries:\n  'https://nexus.abc.de/repository/mode2-npm-hosted/':\n    scopes: ['@pong']\n",
    );
    set_fake_env(&[(
        "pnpm_config__auth",
        r#"{"https://nexus.abc.de/repository/mode2-npm-hosted/": { "@": { "authToken": "token-pong" } }}"#,
    )]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.registry, "https://nexus.abc.de/repository/npm-public/");
    assert_eq!(
        config.registries_by_scope.get("@pong").map(String::as_str),
        Some("https://nexus.abc.de/repository/mode2-npm-hosted/"),
    );
    assert_eq!(
        config
            .resolved_registries()
            .get("default")
            .map(String::as_str),
        Some("https://nexus.abc.de/repository/npm-public/"),
    );
}

/// When default registry was explicitly declared in `.npmrc` and `pnpm_config__auth`
/// defines multiple unscoped registries, the declared default registry is preserved.
#[test]
pub fn json_env_preserves_declared_default_when_multiple_unscoped_registries_present() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join(".npmrc"),
        "registry=https://nexus.abc.de/repository/npm-public/\n",
    );
    set_fake_env(&[(
        "pnpm_config__auth",
        r#"{
            "https://nexus.abc.de/repository/npm-public/": { "@": { "authToken": "tok-1" } },
            "https://other.example.com/": { "@": { "authToken": "tok-2" } }
        }"#,
    )]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.registry, "https://nexus.abc.de/repository/npm-public/");
    assert_eq!(
        config.registries_by_scope.get("default").map(String::as_str),
        Some("https://nexus.abc.de/repository/npm-public/"),
    );
}

/// An `@` credential in the global `config.yaml` `_auth` for a registry the
/// workspace assigns to a scope does not make that registry the default.
#[test]
pub fn global_config_auth_for_scoped_registry_does_not_become_default() {
    fake_env!(load_with_fake_env);
    let xdg = tempdir().expect("xdg tempdir");
    let config_dir = xdg.path().join("pnpm");
    std::fs::create_dir_all(&config_dir).expect("create config dir");
    write_file(
        &config_dir.join("config.yaml"),
        "_auth:\n  \"https://hosted.example\":\n    \"@\":\n      authToken: stored-token\n",
    );
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join("pnpm-workspace.yaml"),
        "registries:\n  'https://hosted.example/':\n    scopes: ['@abc']\n",
    );
    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.registry, "https://registry.npmjs.org/");
    assert_eq!(
        config.registries_by_scope.get("@abc").map(String::as_str),
        Some("https://hosted.example/"),
    );
    assert_eq!(
        config.auth_headers.for_url("https://hosted.example/@abc/pkg").as_deref(),
        Some("Bearer stored-token"),
    );
}

/// A scope the `_auth` env var re-routes no longer claims its old registry,
/// so an `@` credential for that registry can make it the default again.
#[test]
pub fn json_env_rerouted_scope_frees_its_old_registry_for_the_default() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(
        &project.path().join("pnpm-workspace.yaml"),
        "registries:\n  'https://old-scope.example/':\n    scopes: ['@abc']\n",
    );
    set_fake_env(&[(
        "pnpm_config__auth",
        r#"{
            "https://new-scope.example/": { "@abc": { "authToken": "token-abc" } },
            "https://old-scope.example/": { "@": { "authToken": "token-default" } }
        }"#,
    )]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.registry, "https://old-scope.example/");
    assert_eq!(
        config.registries_by_scope.get("@abc").map(String::as_str),
        Some("https://new-scope.example/"),
    );
}
