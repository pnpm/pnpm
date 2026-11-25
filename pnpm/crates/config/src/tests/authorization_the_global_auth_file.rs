use super::{
    Config,
    EnvVar,
    EnvVarOs,
    GetCurrentDir,
    GetHomeDir,
    LinkProbe,
    OsString,
    Path,
    PathBuf,
    STORED_LOGIN,
    assert_eq,
    fs,
    io,
    load_with_auth_file,
    load_with_auth_file_and_npmrc,
    tempdir,
};

#[test]
fn npm_alias_normalizes_workspace_routes_and_json_credentials() {
    for credential_host in ["registry.npmjs.com", "registry.npmjs.org"] {
        let auth = format!(
            "_auth:\n  https://{credential_host}/:\n    '@org': {{ authToken: stored-token }}\n",
        );
        let config = load_with_auth_file(
            &auth,
            Some("registries:\n  https://registry.npmjs.com/:\n    scopes: ['@', '@org']\n"),
        );
        assert_eq!(config.registry, "https://registry.npmjs.org/");
        assert_eq!(config.registries_by_scope["@org"], "https://registry.npmjs.org/");
        assert_eq!(
            config.auth_headers.for_url_with_package(
                "https://registry.npmjs.org/",
                Some("@org/pkg")
            ),
            Some("Bearer stored-token".to_owned()),
        );
        let original_header =
            (credential_host == "registry.npmjs.com").then(|| "Bearer stored-token".to_owned());
        assert_eq!(
            config.auth_headers.for_url_with_package(
                "https://registry.npmjs.com/custom/pkg.tgz",
                Some("@org/pkg"),
            ),
            original_header,
        );
    }
}

/// Only what something else declares is kept back; the stored credential
/// still supplies a route nothing competes for.
#[test]
pub fn the_global_auth_file_routes_what_nothing_else_declares() {
    let config = load_with_auth_file(STORED_LOGIN, None);

    assert_eq!(config.registry, "https://private.example/");
    assert_eq!(
        config.registries_by_scope.get("@org").map(String::as_str),
        Some("https://private.example/"),
    );
}

/// Whether a registry was declared is a question about the key, not its
/// value: pinning the one a lower layer already resolved to is still a
/// declaration, and a stored credential must not quietly replace it.
#[test]
pub fn a_registry_pinned_to_the_default_still_beats_the_global_auth_file() {
    let config = load_with_auth_file(STORED_LOGIN, Some("registry: https://registry.npmjs.org/\n"));

    assert_eq!(config.registry, "https://registry.npmjs.org/");
}

/// A `registries` map naming the default registry declares it as plainly as
/// a `registry:` does, and is recorded under a different key, so asking only
/// about `registry` would miss it.
#[test]
pub fn a_registries_map_declaring_the_default_beats_the_global_auth_file() {
    let config = load_with_auth_file(
        STORED_LOGIN,
        Some("registries:\n  https://declared.example/:\n    scopes: ['@']\n"),
    );

    assert_eq!(config.registry, "https://declared.example/");
}

/// A `registry=` in the project's `.npmrc` declares where packages come from
/// as plainly as a yaml does, so a credential stored for another registry
/// must not redirect the install to it (pnpm/pnpm#14614).
#[test]
pub fn an_npmrc_registry_beats_the_global_auth_file() {
    let config =
        load_with_auth_file_and_npmrc(STORED_LOGIN, "registry=https://project-choice.example/\n");

    assert_eq!(config.registry, "https://project-choice.example/");
    // The credential still reaches the registry it was written for, and
    // does not follow the install to the one the `.npmrc` chose.
    assert_eq!(
        config.auth_tokens_by_uri.get("//private.example/").map(String::as_str),
        Some("stored-token"),
    );
    assert_eq!(
        config.auth_headers.for_url("https://private.example/is-positive").as_deref(),
        Some("Bearer stored-token"),
    );
    assert_eq!(config.auth_headers.for_url("https://project-choice.example/is-positive"), None);
}

#[test]
pub fn an_npmrc_scope_route_beats_the_global_auth_file() {
    let config =
        load_with_auth_file_and_npmrc(STORED_LOGIN, "@org:registry=https://from-npmrc.example/\n");

    assert_eq!(
        config.registries_by_scope.get("@org").map(String::as_str),
        Some("https://from-npmrc.example/"),
    );
    // The default registry is not declared, so the stored credential still routes it.
    assert_eq!(config.registry, "https://private.example/");
    assert_eq!(
        config.auth_headers
            .for_url_with_package("https://private.example/@org%2Fpkg", Some("@org/pkg"))
            .as_deref(),
        Some("Bearer stored-org-token"),
    );
    assert_eq!(
        config.auth_headers.for_url_with_package(
            "https://from-npmrc.example/@org%2Fpkg",
            Some("@org/pkg")
        ),
        None,
    );
}

/// The trusted `.npmrc` an `npmrcAuthFile` names reaches the bootstrap
/// cascade, so its declared registry holds the stored credential's route
/// back there too.
#[test]
pub fn an_npmrc_registry_beats_the_global_auth_file_in_the_bootstrap() {
    fake_env!(load_with_fake_env);
    let xdg = tempdir().expect("xdg tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(config_dir.join("config.yaml"), STORED_LOGIN).expect("write global config.yaml");
    let auth = tempdir().expect("auth tempdir");
    let auth_file = auth.path().join("custom-npmrc");
    fs::write(&auth_file, "registry=https://user-choice.example/\n").expect("write auth file");

    let project = tempdir().expect("project tempdir");
    set_fake_env(&[
        ("XDG_CONFIG_HOME", xdg.path().to_str().unwrap()),
        ("PNPM_CONFIG_NPMRC_AUTH_FILE", auth_file.to_str().unwrap()),
    ]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.registry, "https://user-choice.example/");
    assert_eq!(config.package_manager_bootstrap.registry, "https://user-choice.example/");
    let bootstrap_headers = &config.package_manager_bootstrap.auth_headers;
    assert_eq!(
        bootstrap_headers.for_url("https://private.example/@pnpm%2Fexe").as_deref(),
        Some("Bearer stored-token"),
    );
    assert_eq!(bootstrap_headers.for_url("https://user-choice.example/@pnpm%2Fexe"), None);
}

/// The older `registries: { default: … }` spelling names the default
/// registry as much as a declaration routing the bare `@` does, and the
/// reader treats it that way, so the declaration must be seen through it too.
#[test]
pub fn a_legacy_default_registries_key_beats_the_global_auth_file() {
    let config = load_with_auth_file(
        STORED_LOGIN,
        Some("registries:\n  default: https://legacy-declared.example/\n"),
    );

    assert_eq!(config.registry, "https://legacy-declared.example/");
}

#[test]
fn http_tarball_auth_requires_a_user_declared_registry() {
    let routes = "registries:\n  http://private.example/npm/: { scopes: ['@'] }\n";
    let first = "http://private.example/npm/pkg.tgz";
    let global = load_with_auth_file(&format!("{STORED_LOGIN}{routes}"), None);
    let authorize = global.auth_headers.tarball_authorization(first, "pkg");
    assert_eq!(authorize(first).as_deref(), Some("Bearer stored-token"));
    assert_eq!(authorize("http://private.example/npm/cache/pkg.tgz"), authorize(first));
    assert_eq!(authorize("http://private.example/npm-two/pkg.tgz"), None);
    assert_eq!(authorize("http://private.example:8080/npm/pkg.tgz"), None);

    let strict = (*global.auth_headers).clone().with_secure_transport();
    assert_eq!(strict.tarball_authorization(first, "pkg")(first), None);
    assert_eq!(global.auth_headers.tarball_authorization(first, "node@runtime:1")(first), None);
    assert_eq!(authorize("http://other.example/npm/pkg.tgz"), None);

    let project = load_with_auth_file(STORED_LOGIN, Some(routes));
    assert_eq!(project.auth_headers.tarball_authorization(first, "pkg")(first), None);
    let project_ini =
        load_with_auth_file_and_npmrc(STORED_LOGIN, "registry=http://private.example/npm/");
    assert_eq!(project_ini.auth_headers.tarball_authorization(first, "pkg")(first), None);
    let https = "https://private.example/npm/pkg.tgz";
    assert_eq!(global.auth_headers.tarball_authorization(https, "pkg")(first), None);
}

#[test]
fn http_tarball_auth_uses_only_effective_trusted_registry_aliases() {
    let alias = "namedRegistries:\n  custom: http://private.example/npm/\n";
    let first = "http://private.example/npm/pkg.tgz";
    let accepted = load_with_auth_file(&format!("{STORED_LOGIN}{alias}"), None);
    assert_eq!(
        accepted.auth_headers.tarball_authorization(first, "pkg")(first).as_deref(),
        Some("Bearer stored-token"),
    );

    for declarations in [
        "registries:\n  http://private.example/npm/: { ecosystem: pypi }\n",
        "registries:\n  https://canonical.example/: { prefix: custom }\n",
    ] {
        let config = load_with_auth_file(&format!("{STORED_LOGIN}{declarations}{alias}"), None);
        assert_eq!(config.auth_headers.tarball_authorization(first, "pkg")(first), None);
    }
    let project = load_with_auth_file(STORED_LOGIN, Some(alias));
    assert_eq!(project.auth_headers.tarball_authorization(first, "pkg")(first), None);
}

#[test]
fn both_user_auth_files_can_declare_http_registries() {
    fake_env!(load_with_fake_env);
    let root = tempdir().unwrap();
    let config_dir = root.path().join("pnpm");
    fs::create_dir_all(&config_dir).unwrap();
    let user = root.path().join("user.npmrc");
    super::write_registry_auth_file(&user, "http://first.example/", "first");
    super::write_registry_auth_file(
        &config_dir.join("auth.ini"),
        "http://second.example/",
        "second",
    );
    set_fake_env(&[
        ("XDG_CONFIG_HOME", root.path().to_str().unwrap()),
        ("PNPM_CONFIG_NPMRC_AUTH_FILE", user.to_str().unwrap()),
    ]);
    let project = tempdir().unwrap();
    let config = load_with_fake_env(project.path());
    assert_eq!(config.registry, "http://second.example/");
    for (url, token) in [
        ("http://first.example/pkg.tgz", "Bearer first"),
        ("http://second.example/pkg.tgz", "Bearer second"),
    ] {
        assert_eq!(
            config.auth_headers.tarball_authorization(url, "pkg")(url).as_deref(),
            Some(token),
        );
    }
}
