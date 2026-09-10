use super::{
    Config, EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, LinkProbe, OsString, Path, PathBuf,
    STORED_LOGIN, assert_eq, fs, io, load_with_auth_file, load_with_auth_file_and_npmrc, tempdir,
};

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
        config
            .auth_headers
            .for_url_with_package("https://private.example/@org%2Fpkg", Some("@org/pkg"))
            .as_deref(),
        Some("Bearer stored-org-token"),
    );
    assert_eq!(
        config
            .auth_headers
            .for_url_with_package("https://from-npmrc.example/@org%2Fpkg", Some("@org/pkg")),
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
