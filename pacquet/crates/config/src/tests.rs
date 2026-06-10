use super::{
    Config, EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, Host, LinkProbe, NodeLinker,
    PackageImportMethod, fs,
};
use crate::defaults::default_store_dir;
use pacquet_store_dir::StoreDir;
use pacquet_testing_utils::env_guard::EnvGuard;
use pretty_assertions::assert_eq;
use std::{
    env,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};
use tempfile::tempdir;

/// `Config::current` requires `Sys: LinkProbe` so the late-stage
/// `store_dir` resolver (port of pnpm's `storePathRelativeToHome`)
/// can probe linkability between project and home. Tests in this
/// module pin specific config-cascade behaviours, none of which
/// turn on cross-volume detection, so the test fakes return
/// `false` for every probe. The probe failing collapses to the
/// pre-existing `SmartDefault` `store_dir` value, which is what the
/// pre-port assertions already assume.
///
/// `inert_link_probe!(Name)` wires the impl onto a local test
/// fake without polluting each test fn with the boilerplate.
macro_rules! inert_link_probe {
    ($($t:ty),+ $(,)?) => {$(
        impl LinkProbe for $t {
            fn can_link_between_dirs(_: &Path, _: &Path) -> bool {
                false
            }
        }
    )+};
}

fn display_store_dir(store_dir: &StoreDir) -> String {
    store_dir.display().to_string().replace('\\', "/")
}

/// Delegate to [`Host::var`] but mask the env vars that would
/// otherwise let the developer's real shell steer pacquet's global
/// `config.yaml` loader or its `PNPM_CONFIG_*` overlay:
///
/// - `XDG_CONFIG_HOME` / `LOCALAPPDATA` — both feed
///   [`crate::defaults::default_config_dir`], so a value set on the
///   dev box would point the global-config loader at a real
///   `config.yaml` on disk.
/// - `PNPM_CONFIG_*` / `pnpm_config_*` — the env-var overlay reads
///   the entire schema, so a stray `PNPM_CONFIG_ENABLE_GLOBAL_VIRTUAL_STORE`
///   in the developer's shell would flip GVS in every test that
///   otherwise expects defaults.
///
/// Tests that exercise those code paths declare per-test fakes
/// that satisfy [`EnvVar`] with their own response logic — they
/// don't go through this helper.
fn safe_host_var(name: &str) -> Option<String> {
    if name == "XDG_CONFIG_HOME" || name == "LOCALAPPDATA" {
        return None;
    }
    if name.starts_with("PNPM_CONFIG_") || name.starts_with("pnpm_config_") {
        return None;
    }
    Host::var(name)
}

/// Common test [`crate::api::Sys`]-shaped fake: env reads delegate
/// to [`Host`], home dir resolves to `None`. Lets a test exercise
/// `Config::current`'s `.npmrc`-in-`start_dir` and yaml-walk paths
/// without consulting the developer's real home directory.
struct HostNoHome;
impl EnvVar for HostNoHome {
    fn var(name: &str) -> Option<String> {
        safe_host_var(name)
    }
}
impl EnvVarOs for HostNoHome {
    fn var_os(_: &str) -> Option<OsString> {
        // Return `None` rather than delegating to [`Host`] so an
        // ambient `NPM_CONFIG_WORKSPACE_DIR` on a developer
        // machine can't steer unrelated tests into the env-var
        // workspace-dir branch. Tests that exercise that branch
        // declare their own [`EnvVarOs`] fakes.
        None
    }
}
impl GetHomeDir for HostNoHome {
    fn home_dir() -> Option<PathBuf> {
        None
    }
}
inert_link_probe!(HostNoHome);

#[test]
pub fn have_default_values() {
    let value = Config::new();
    assert_eq!(value.node_linker, NodeLinker::default());
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
    assert_eq!(value.registry, "https://registry.npmjs.org/");
}

/// `fetch-retries*` defaults must match pnpm's
/// `config/config/src/index.ts` (`2`, `10`, `10000`, `60000`) — these
/// are the values pnpm bakes into npm-style fetches and we want
/// pacquet to behave identically out of the box.
#[test]
pub fn fetch_retries_defaults_match_pnpm() {
    let value = Config::new();
    assert_eq!(value.fetch_retries, 2);
    assert_eq!(value.fetch_retry_factor, 10);
    assert_eq!(value.fetch_retry_mintimeout, 10_000);
    assert_eq!(value.fetch_retry_maxtimeout, 60_000);
}

/// Network knobs default to pnpm's values: `networkConcurrency`
/// from the shared formula, `fetchTimeout` at 60 000 ms, a
/// `pnpm/…`-shaped `userAgent`, and no `npmrcAuthFile` override.
#[test]
pub fn network_settings_defaults_match_pnpm() {
    let value = Config::new();
    assert_eq!(value.network_concurrency, pacquet_network::default_network_concurrency());
    assert_eq!(value.fetch_timeout, 60_000);
    assert!(value.user_agent.starts_with("pnpm/"), "user-agent: {:?}", value.user_agent);
    assert_eq!(value.npmrc_auth_file, None);
}

/// `npmrcAuthFile` redirects the user-level `.npmrc` read: auth
/// from the pointed-at file reaches `auth_headers` even though the
/// file is neither at `~/.npmrc` (home resolves to `None` here) nor
/// in `start_dir`.
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

thread_local! {
    /// Per-thread fake environment for the `npmrcAuthFile` env-var
    /// resolution tests, set via [`set_fake_env`]. Lets a single
    /// `Sys` fake ([`FakeEnv`]) serve every precedence test without
    /// mutating the real process environment (no `set_var` / no
    /// `EnvGuard` lock). `cargo test` runs each test on its own
    /// thread, so the maps don't collide.
    static FAKE_ENV: std::cell::RefCell<std::collections::HashMap<String, String>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

fn set_fake_env(pairs: &[(&str, &str)]) {
    FAKE_ENV.with(|map| {
        let mut map = map.borrow_mut();
        map.clear();
        for (key, value) in pairs {
            map.insert((*key).to_string(), (*value).to_string());
        }
    });
}

/// `Sys` fake whose env reads come from the thread-local
/// [`FAKE_ENV`] (and nothing else), with no home dir. Isolates the
/// `npmrcAuthFile` env-resolution from the developer's real shell.
struct FakeEnv;
impl EnvVar for FakeEnv {
    fn var(name: &str) -> Option<String> {
        FAKE_ENV.with(|map| map.borrow().get(name).cloned())
    }
}
impl EnvVarOs for FakeEnv {
    fn var_os(_: &str) -> Option<OsString> {
        None
    }
}
impl GetHomeDir for FakeEnv {
    fn home_dir() -> Option<PathBuf> {
        None
    }
}
inert_link_probe!(FakeEnv);

/// Write a `.npmrc` that declares its own registry plus an unscoped
/// `_authToken`, so the token pins to that registry — the shape the
/// precedence assertions check the winning file by.
fn write_registry_auth_file(path: &Path, registry: &str, token: &str) {
    fs::write(path, format!("registry={registry}\n_authToken={token}\n")).expect("write auth file");
}

fn load_with_fake_env(start_dir: &Path) -> Config {
    Config::default().current::<FakeEnv>(start_dir).expect("load config")
}

/// `PNPM_CONFIG_NPMRC_AUTH_FILE` (uppercase) resolves the user-level
/// `.npmrc`. Mirrors pnpm's `readEnvVar(env, 'npmrc_auth_file')`.
#[test]
pub fn npmrc_auth_file_from_pnpm_config_env() {
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

/// The lowercase `pnpm_config_npmrc_auth_file` is honoured too —
/// pnpm's `readEnvVar` accepts both cases.
#[test]
pub fn npmrc_auth_file_from_lowercase_pnpm_config_env() {
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

/// An exported-but-empty `PNPM_CONFIG_NPMRC_AUTH_FILE` must fall
/// through to `PNPM_CONFIG_USERCONFIG` rather than short-circuiting
/// the resolution, matching pnpm's per-variable `value !== ''`
/// filter.
#[test]
pub fn npmrc_auth_file_empty_env_falls_through_to_userconfig() {
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

/// `npmrc_auth_file` outranks `userconfig` (pnpm resolves
/// `readEnvVar('npmrc_auth_file')` before `readEnvVar('userconfig')`).
#[test]
pub fn npmrc_auth_file_outranks_userconfig() {
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

/// `npm_config_userconfig` is honoured as a low-priority npm
/// compatibility fallback (e.g. `actions/setup-node`), and a
/// `PNPM_CONFIG_*` value outranks it.
#[test]
pub fn npmrc_auth_file_npm_config_userconfig_is_compat_fallback() {
    let project = tempdir().expect("project tempdir");
    let auth = tempdir().expect("auth tempdir");
    let npm_file = auth.path().join("npm-userconfig");
    write_registry_auth_file(&npm_file, "https://npm.example.com/", "npm-token");

    // Compat fallback alone resolves.
    set_fake_env(&[("npm_config_userconfig", npm_file.to_str().unwrap())]);
    let config = load_with_fake_env(project.path());
    assert_eq!(
        config.auth_headers.for_url("https://npm.example.com/pkg").as_deref(),
        Some("Bearer npm-token"),
    );

    // A pnpm-native value wins over the npm compat fallback.
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

#[test]
pub fn global_config_yaml_request_destination_values_expand_env() {
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
        config.named_registries.get("work").map(String::as_str),
        Some("https://trusted.example.com/work/"),
    );
}

#[test]
pub fn pnpm_config_request_destinations_expand_env() {
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

fn write_file(path: &Path, contents: &str) {
    fs::write(path, contents).expect("write file");
}

/// Load `Config` with a project `.npmrc` in `start_dir` plus a
/// user-level file pointed at by `npmrcAuthFile`. Home resolves to
/// `None` ([`HostNoHome`]) so only these two files participate —
/// the multi-file merge + per-file rescoping under test.
fn load_with_project_and_user(project_npmrc: &str, user_file: PathBuf) -> Config {
    let project = tempdir().expect("project tempdir");
    write_file(&project.path().join(".npmrc"), project_npmrc);
    Config { npmrc_auth_file: Some(user_file), ..Config::default() }
        .current::<HostNoHome>(project.path())
        .expect("load config")
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

/// `_auth` (basic) is pinned the same way.
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

/// `username` + `_password` are pinned the same way.
#[test]
pub fn user_username_password_pins_to_its_own_file_registry() {
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    // _password is base64("pass")
    write_file(
        &user_file,
        "registry=https://trusted.example.com/\nusername=alice\n_password=cGFzcw==\n",
    );

    let config = load_with_project_and_user("registry=https://attacker.example.com/\n", user_file);

    let expected = format!("Basic {}", pacquet_network::base64_encode("alice:pass"));
    assert_eq!(
        config.auth_headers.for_url("https://trusted.example.com/pkg").as_deref(),
        Some(expected.as_str()),
    );
    assert_eq!(config.auth_headers.for_url("https://attacker.example.com/pkg"), None);
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

/// `auth.ini` (in the global config dir) with no `registry=` of its
/// own falls back to the npmjs default for its unscoped creds — it
/// does not borrow the user file's or workspace's registry.
#[test]
pub fn auth_ini_without_registry_falls_back_to_npmjs_default() {
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

/// `default_store_dir`'s `PNPM_HOME` branch, exercised through the
/// generic capability seam — no process-environment mutation, no
/// `EnvGuard` lock, no `unsafe` block. The earlier shape of this
/// test set `PNPM_HOME` via `std::env::set_var` and called
/// `Config::new()`; with the DI seam from pnpm/pacquet#339 +
/// pnpm/pnpm#11708 the same precedence is checked by passing a
/// per-test unit struct that satisfies [`EnvVar`], [`GetHomeDir`],
/// and [`GetCurrentDir`].
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
        format!("/hello/store/{}", pacquet_store_dir::STORE_VERSION),
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
        format!("/hello/pnpm/store/{}", pacquet_store_dir::STORE_VERSION),
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

/// pnpm 11's `isIniConfigKey` (config/config/src/auth.ts) leaves the
/// `fetch-retries*` family out of `NPM_AUTH_SETTINGS`, so a value
/// like `fetch-retries=99` in `.npmrc` is silently ignored upstream.
/// pacquet must do the same — applying it would diverge from pnpm
/// and silently change install behaviour for projects that have a
/// stale `.npmrc` lying around.
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
    // write invalid utf-8 value to npmrc
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
    let config = Config::new()
        .current::<HostWithHome>(current_dir.path())
        .expect("workspace yaml absent => no error");
    assert_eq!(config.registry, "https://home.example/");
}

#[test]
pub fn pnpm_workspace_yaml_registry_overrides_npmrc_registry() {
    // `registry` is the one non-scope key pnpm 11 still reads from
    // .npmrc (it's in RAW_AUTH_CFG_KEYS). When both files define it,
    // the yaml wins, matching pnpm itself.
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join(".npmrc"), "registry=https://from-npmrc.test")
        .expect("write to .npmrc");
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "registry: https://from-yaml.test\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert_eq!(config.registry, "https://from-yaml.test/");
}

#[test]
pub fn pnpm_workspace_yaml_found_by_walking_up() {
    let tmp = tempdir().unwrap();
    let nested = tmp.path().join("packages/inner");
    fs::create_dir_all(&nested).unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "symlink: false\n")
        .expect("write to pnpm-workspace.yaml");
    // No `.npmrc` anywhere, but a parent dir has `pnpm-workspace.yaml` —
    // the yaml should still be applied.
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
    let config = Config { symlink: false, ..Config::new() }
        .current::<HostWithHome>(current_dir.path())
        .expect("workspace yaml absent => no error");
    assert!(!config.symlink);
}

/// `enableGlobalVirtualStore` defaults to `false` — matches pnpm
/// v11's effective default for regular installs (the `true`
/// assignment lives only inside the `--global` install branch;
/// see [`Config::enable_global_virtual_store`]). The derivation
/// still fires automatically from [`Config::current`] after yaml
/// has been applied, writing `<store_dir>/links` into
/// `global_virtual_store_dir` while leaving `virtual_store_dir`
/// at its project-local default — both fields stay valid so the
/// downstream code can read either one without first checking
/// the toggle. Pacquet's split-field variant of upstream's
/// [`extendInstallOptions.ts:343-355`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-installer/src/install/extendInstallOptions.ts#L343-L355);
/// see [`Config::apply_global_virtual_store_derivation`] for why
/// pacquet keeps them separate.
#[test]
pub fn gvs_default_is_off_and_paths_derive_cleanly() {
    let tmp = tempdir().unwrap();
    let config =
        Config::new().current::<HostNoHome>(tmp.path()).expect("workspace yaml absent => no error");
    assert!(
        !config.enable_global_virtual_store,
        "GVS defaults to false (matches pnpm v11 for non-global installs)",
    );
    // `virtual_store_dir` stays project-local. The
    // `<cwd>/node_modules/.pnpm` default has been re-anchored to
    // `tmp` by `Config::current` (see the cwd fixup block).
    assert_eq!(config.virtual_store_dir, tmp.path().join("node_modules/.pnpm"));
    assert_eq!(config.global_virtual_store_dir, config.store_dir.links());
}

/// When `enableGlobalVirtualStore: false`, the derivation still
/// populates `global_virtual_store_dir` at `<storeDir>/links` —
/// matching upstream's unconditional `globalVirtualStoreDir =
/// storeDir/links` assignment for the GVS-off branch. The frozen-
/// lockfile install path consults `enable_global_virtual_store`
/// to decide whether to consume the field.
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

/// When the user pins `virtualStoreDir` *and* opts into GVS,
/// `globalVirtualStoreDir` mirrors that path — the user gets to
/// pick where the shared store lives. `virtual_store_dir` itself
/// still holds the pinned value (it's the same field the user
/// configured).
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

/// An explicit `globalVirtualStoreDir` in yaml wins over the
/// derivation: the resolved field equals the user-supplied path,
/// not `<store_dir>/links` and not the user's `virtualStoreDir`.
/// Mirrors upstream's
/// [`getOptionsFromRootManifest.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/getOptionsFromRootManifest.ts)
/// — `globalVirtualStoreDir` is read into the config there with
/// the same resolve-relative-to-workspace semantics. Without this
/// preservation the value parses from yaml and then gets
/// silently overwritten by the derivation.
///
/// The fixture also enables GVS explicitly. Pacquet's default
/// is `enableGlobalVirtualStore: false` (matches pnpm v11 for
/// non-`--global` installs), so without the explicit opt-in the
/// GVS-on derivation path wouldn't run at all and the test
/// would say nothing about that path's behaviour.
#[test]
pub fn yaml_global_virtual_store_dir_wins_over_derivation() {
    let tmp = tempdir().unwrap();
    let yaml_gvs = tmp.path().join("my-shared-store");
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        format!("enableGlobalVirtualStore: true\nglobalVirtualStoreDir: {}\n", yaml_gvs.display()),
    )
    .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert!(config.enable_global_virtual_store);
    // `virtual_store_dir` stays at the project-local default,
    // because the user didn't pin `virtualStoreDir`.
    assert_eq!(config.virtual_store_dir, tmp.path().join("node_modules/.pnpm"));
    // The explicit yaml value wins — neither the derivation's
    // `<store_dir>/links` fallback nor any mirroring of
    // `virtual_store_dir` clobbers it.
    assert_eq!(config.global_virtual_store_dir, yaml_gvs);
}

/// Real-process-environment smoke test for the proxy env-var
/// fallback through `Config::current`. The injected-`EnvVar` tests
/// in `npmrc_auth/tests.rs` cover the cascade branches
/// exhaustively; this one only proves the wiring through
/// `Host::var` reaches `std::env::var` and that the cascade
/// fires even with no `.npmrc` present.
#[test]
pub fn proxy_env_fallback_applies_through_current() {
    // Snapshot every proxy var the cascade might read so peer tests
    // can't observe our mutations and so the env restores cleanly.
    let _g = EnvGuard::snapshot([
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "PROXY",
        "proxy",
        "NO_PROXY",
        "no_proxy",
        "NPM_CONFIG_WORKSPACE_DIR",
        "npm_config_workspace_dir",
    ]);
    let tmp = tempdir().unwrap();
    // SAFETY: EnvGuard above serializes the test against other
    // env-mutating tests in this process; no other thread reads
    // these vars concurrently. The other proxy vars are removed so
    // a host-set value can't leak in and skew the assertion.
    unsafe {
        env::remove_var("HTTPS_PROXY");
        env::remove_var("https_proxy");
        env::remove_var("HTTP_PROXY");
        env::remove_var("http_proxy");
        env::remove_var("PROXY");
        env::remove_var("proxy");
        env::remove_var("NO_PROXY");
        env::remove_var("no_proxy");
        env::remove_var("NPM_CONFIG_WORKSPACE_DIR");
        env::remove_var("npm_config_workspace_dir");
        env::set_var("HTTPS_PROXY", "http://env.example:8080");
    }
    let config =
        Config::new().current::<HostNoHome>(tmp.path()).expect("workspace yaml absent => no error");
    assert_eq!(config.proxy.https_proxy.as_deref(), Some("http://env.example:8080"));
    assert_eq!(
        config.proxy.http_proxy.as_deref(),
        Some("http://env.example:8080"),
        "http side cascades through resolved https",
    );
}

/// Pnpm's
/// [`workspace-manifest-reader`](https://github.com/pnpm/pnpm/blob/8eb1be4988/workspace/workspace-manifest-reader/src/index.ts)
/// fails the process on invalid yaml. `Config::current` must do the
/// same instead of silently falling back to defaults.
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
/// matching pnpm v11's `pnpmConfig.dir = lockfileDir` rule. Without
/// this, the per-importer `node_modules` writes (under the
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

/// A single-project install (no `pnpm-workspace.yaml` anywhere)
/// keeps the CLI `--dir` as the anchor. Guards against the
/// re-anchor block accidentally firing when no workspace exists.
///
/// [`HostNoHome`] already pins the `NPM_CONFIG_WORKSPACE_DIR`
/// lookup to `None`, so the test never reads the host's real
/// environment. Replaces the earlier shape that snapshotted both
/// spellings of the env variable through `EnvGuard` and called
/// `unsafe { env::remove_var(...) }`.
#[test]
pub fn single_project_anchors_modules_at_cwd() {
    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("config loads");
    assert_eq!(config.modules_dir, tmp.path().join("node_modules"));
    assert_eq!(config.virtual_store_dir, tmp.path().join("node_modules/.pnpm"));
}

/// `NPM_CONFIG_WORKSPACE_DIR` must steer `Config::current`'s
/// path-anchoring just like it steers
/// [`pacquet_workspace::find_workspace_dir`] — otherwise the
/// virtual store would land in the cwd while the per-importer
/// `SymlinkDirectDependencies` writes land under the env-var
/// path, producing two `node_modules` layouts for the same
/// install. Matches the consistency guarantee Copilot flagged
/// during PR [#443](https://github.com/pnpm/pacquet/pull/443) review.
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
/// upward walk, matching pnpm's truthy `if (workspaceDir)` check.
/// Pairs with `pacquet_workspace`'s
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
    let tmp = tempdir().unwrap();
    let config =
        Config::new().current::<HostWithEmptyEnvWorkspaceDir>(tmp.path()).expect("config loads");
    // No yaml in tmp → no re-anchor → cwd-anchored defaults.
    assert_eq!(config.modules_dir, tmp.path().join("node_modules"));
    assert_eq!(config.virtual_store_dir, tmp.path().join("node_modules/.pnpm"));
}

/// `enableGlobalVirtualStore: true` set in the global
/// `<configDir>/config.yaml` is honored by `Config::current` —
/// the exact scenario from pnpm/pnpm#11738 where a user has
/// the setting in `~/.config/pnpm/config.yaml` and runs an
/// install in a project whose `pnpm-workspace.yaml` doesn't
/// repeat it.
///
/// Drives the [`EnvVar`] + [`GetHomeDir`] DI seams: the fake
/// returns the test's tempdir for `XDG_CONFIG_HOME`, so
/// [`crate::defaults::default_config_dir`] resolves to
/// `<tempdir>/pnpm/config.yaml` rather than touching the
/// developer's real config dir.
#[test]
pub fn global_config_yaml_enables_gvs() {
    let xdg = tempdir().unwrap();
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("config.yaml"), "enableGlobalVirtualStore: true\n")
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
            // XDG_CONFIG_HOME short-circuits the home_dir lookup
            // inside `default_config_dir`, but `Config::current`'s
            // home-dir `.npmrc` fallback still consults it. The
            // fallback gracefully tolerates `None`, so returning
            // `None` keeps the test hermetic without forcing a
            // tempdir for the unrelated `.npmrc` path.
            None
        }
    }
    inert_link_probe!(HostWithXdgConfigHome);

    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostWithXdgConfigHome>(tmp.path()).expect("config loads");
    assert!(
        config.enable_global_virtual_store,
        "enableGlobalVirtualStore from global config.yaml must apply",
    );
}

/// `pnpm-workspace.yaml` overrides the global `config.yaml` —
/// global enables GVS, workspace disables it, the install
/// resolves to GVS-off. Matches pnpm's
/// [`index.ts:421-444`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/index.ts#L421-L444),
/// which applies workspace yaml after the global yaml.
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

    let config =
        Config::new().current::<HostWithXdgConfigHome>(project.path()).expect("config loads");
    assert_eq!(
        config.virtual_store_dir, global_path,
        "virtualStoreDir from global config.yaml must survive the workspace-root re-anchor",
    );
}

/// Workspace-only keys in the global `config.yaml` are silently
/// ignored, matching pnpm's
/// [`isConfigFileKey`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/configFileKey.ts#L187)
/// filter at
/// [`index.ts:299-309`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/index.ts#L299-L309).
/// A `nodeLinker: hoisted` in the global yaml would change the
/// installer's layout strategy if applied — pnpm rejects it, and
/// pacquet must too.
#[test]
pub fn global_config_yaml_workspace_only_keys_are_ignored() {
    let xdg = tempdir().unwrap();
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("config.yaml"),
        // `nodeLinker`, `hoist`, `symlink`, and `lockfile` are
        // all in pnpm's `excludedPnpmKeys`. None should apply
        // when set in the global config.
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

    let tmp = tempdir().unwrap();
    let defaults = Config::new();
    let config = Config::new().current::<HostWithXdgConfigHome>(tmp.path()).expect("config loads");
    assert_eq!(config.node_linker, defaults.node_linker);
    assert_eq!(config.hoist, defaults.hoist);
    assert_eq!(config.symlink, defaults.symlink);
    assert_eq!(config.lockfile, defaults.lockfile);
}

/// `PNPM_CONFIG_ENABLE_GLOBAL_VIRTUAL_STORE=true` is read into
/// the config — drives the env-overlay introduced alongside
/// global `config.yaml` support. Mirrors pnpm's `parseEnvVars`
/// loop at
/// [`index.ts:471-488`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/index.ts#L471-L488)
/// for the `PNPM_CONFIG_*` family (pnpm doesn't read general
/// `NPM_CONFIG_*` env vars; see
/// [`feedback_pnpm_settings_not_in_npmrc`](https://github.com/pnpm/pnpm/blob/main/config/reader/src/localConfig.ts)).
#[test]
pub fn pnpm_config_env_var_enables_gvs() {
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

    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostWithPnpmConfigEnv>(tmp.path()).expect("loads");
    assert!(config.enable_global_virtual_store);
}

/// Lowercase `pnpm_config_*` spelling also works, matching
/// upstream's
/// [`getEnvKeySuffix`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/config/reader/src/env.ts#L185-L195)
/// which accepts both case forms.
#[test]
pub fn pnpm_config_env_var_lowercase_works() {
    struct HostWithLowercaseEnv;
    impl EnvVar for HostWithLowercaseEnv {
        fn var(name: &str) -> Option<String> {
            if name == "pnpm_config_enable_global_virtual_store" {
                return Some("true".to_owned());
            }
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithLowercaseEnv {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithLowercaseEnv {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithLowercaseEnv);

    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostWithLowercaseEnv>(tmp.path()).expect("loads");
    assert!(config.enable_global_virtual_store);
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

    let config = Config::new().current::<HostWithPnpmConfigEnv>(tmp.path()).expect("loads");
    assert!(
        config.enable_global_virtual_store,
        "PNPM_CONFIG_* env var must win over pnpm-workspace.yaml",
    );
}

/// `PNPM_CONFIG_HOIST=false` runs the same post-processing as
/// yaml-set `hoist: false` — it short-circuits `hoist_pattern`
/// to `None`, mirroring upstream's
/// [`projectConfig.ts:72-75`](https://github.com/pnpm/pnpm/blob/94240bc046/config/reader/src/projectConfig.ts#L72-L75)
/// rule (`hoist === false ⇒ hoistPattern: undefined`). Without
/// this, the install-time `hoist_pattern.is_some() ||
/// public_hoist_pattern.is_some()` guard would still enable
/// hoisting even after the user disabled it via env var.
#[test]
pub fn pnpm_config_hoist_false_clears_hoist_pattern() {
    struct HostWithHoistEnv;
    impl EnvVar for HostWithHoistEnv {
        fn var(name: &str) -> Option<String> {
            if name == "PNPM_CONFIG_HOIST" {
                return Some("false".to_owned());
            }
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithHoistEnv {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithHoistEnv {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithHoistEnv);

    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostWithHoistEnv>(tmp.path()).expect("loads");
    assert!(!config.hoist);
    assert_eq!(
        config.hoist_pattern, None,
        "hoist: false must clear hoist_pattern, even when set via env var",
    );
}

/// `virtualStoreDirMaxLength` defaults to 120 — same value pnpm
/// writes when nothing is configured. The constant lives in
/// `pacquet-modules-yaml`; this asserts the config side carries
/// the matching default so a fresh install produces the same
/// virtual-store dirnames as pnpm.
#[test]
pub fn virtual_store_dir_max_length_defaults_to_120() {
    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");
    assert_eq!(config.virtual_store_dir_max_length, 120);
}

/// `virtualStoreDirMaxLength` in `pnpm-workspace.yaml` overrides
/// the default. Mirrors pnpm's
/// [`virtualStoreDirMaxLength`](https://github.com/pnpm/pnpm/blob/1819226b51/config/reader/src/Config.ts)
/// config-reader entry.
#[test]
pub fn virtual_store_dir_max_length_from_workspace_yaml() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "virtualStoreDirMaxLength: 90\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert_eq!(config.virtual_store_dir_max_length, 90);
}

/// `PNPM_CONFIG_VIRTUAL_STORE_DIR_MAX_LENGTH` overrides the yaml
/// value, matching the reader cascade priority (env > yaml >
/// default).
#[test]
pub fn virtual_store_dir_max_length_env_var_overrides_yaml() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "virtualStoreDirMaxLength: 90\n")
        .expect("write to pnpm-workspace.yaml");

    struct HostWithEnvOverride;
    impl EnvVar for HostWithEnvOverride {
        fn var(name: &str) -> Option<String> {
            if name == "PNPM_CONFIG_VIRTUAL_STORE_DIR_MAX_LENGTH" {
                return Some("50".to_owned());
            }
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithEnvOverride {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithEnvOverride {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithEnvOverride);

    let config = Config::new().current::<HostWithEnvOverride>(tmp.path()).expect("loads");
    assert_eq!(
        config.virtual_store_dir_max_length, 50,
        "env var must win over pnpm-workspace.yaml",
    );
}

/// `peersSuffixMaxLength` defaults to 1000 — same value pnpm
/// uses for `createPeerDepGraphHash`'s `maxLength` parameter when
/// nothing is configured.
#[test]
pub fn peers_suffix_max_length_defaults_to_1000() {
    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");
    assert_eq!(config.peers_suffix_max_length, 1000);
}

/// `peersSuffixMaxLength` in `pnpm-workspace.yaml` overrides the
/// default. Mirrors pnpm's
/// [`peersSuffixMaxLength`](https://github.com/pnpm/pnpm/blob/39101f5e37/config/reader/src/Config.ts#L256)
/// config-reader entry.
#[test]
pub fn peers_suffix_max_length_from_workspace_yaml() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "peersSuffixMaxLength: 10\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert_eq!(config.peers_suffix_max_length, 10);
}

/// `PNPM_CONFIG_PEERS_SUFFIX_MAX_LENGTH` overrides the yaml value,
/// matching the reader cascade priority (env > yaml > default).
#[test]
pub fn peers_suffix_max_length_env_var_overrides_yaml() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "peersSuffixMaxLength: 10\n")
        .expect("write to pnpm-workspace.yaml");

    struct HostWithEnvOverride;
    impl EnvVar for HostWithEnvOverride {
        fn var(name: &str) -> Option<String> {
            if name == "PNPM_CONFIG_PEERS_SUFFIX_MAX_LENGTH" {
                return Some("25".to_owned());
            }
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithEnvOverride {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithEnvOverride {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithEnvOverride);

    let config = Config::new().current::<HostWithEnvOverride>(tmp.path()).expect("loads");
    assert_eq!(config.peers_suffix_max_length, 25, "env var must win over pnpm-workspace.yaml");
}

/// `Config::patched_dependency_hashes` resolves each relative patch
/// path against `workspace_dir`, hashes the file, and returns the
/// verbatim key → hash map the lockfile records. Mirrors pnpm's
/// `calcPatchHashes(opts.patchedDependencies)`.
#[test]
fn patched_dependency_hashes_resolves_and_hashes_each_patch() {
    let workspace = tempdir().expect("workspace tempdir");
    let patch_path = workspace.path().join("patches").join("graceful-fs@4.2.11.patch");
    std::fs::create_dir_all(patch_path.parent().unwrap()).expect("create patches dir");
    std::fs::write(&patch_path, "--- a/index.js\n+++ b/index.js\n").expect("write patch");
    let expected =
        pacquet_patching::create_hex_hash_from_file(&patch_path).expect("hash patch file");

    let mut config = Config::new();
    assert!(config.patched_dependency_hashes().expect("no error").is_none(), "unset → None");

    config.workspace_dir = Some(workspace.path().to_path_buf());
    config.patched_dependencies = Some(indexmap::IndexMap::from([(
        "graceful-fs@4.2.11".to_string(),
        "patches/graceful-fs@4.2.11.patch".to_string(),
    )]));
    let hashes = config.patched_dependency_hashes().expect("hash").expect("present");
    assert_eq!(hashes.get("graceful-fs@4.2.11"), Some(&expected));
}

/// `minimumReleaseAge: 0` disables the maturity cutoff (returns
/// `None`), matching pnpm's falsy check; any positive value passes
/// through, and the built-in default (1440) is non-zero.
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
