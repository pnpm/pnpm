use super::{
    Config, EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, Host, LinkProbe, LoadWorkspaceYamlError,
    NodeLinker, NodePackageMapType, PackageImportMethod, fs,
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
    sync::{Arc, Mutex},
};
use tempfile::tempdir;
use tracing::Level;
use tracing_subscriber::{Layer, layer::SubscriberExt};

/// Capture all tracing WARN messages emitted during a closure.
fn capture_warnings<Func: FnOnce()>(f: Func) -> Vec<String> {
    let messages: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let messages_clone = Arc::clone(&messages);

    struct CaptureLayer(Arc<Mutex<Vec<String>>>);
    impl<Sub: tracing::Subscriber> Layer<Sub> for CaptureLayer {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, Sub>,
        ) {
            if *event.metadata().level() == Level::WARN {
                struct Visitor(String);
                impl tracing::field::Visit for Visitor {
                    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                        if field.name() == "message" {
                            self.0 = value.to_string();
                        }
                    }
                    fn record_debug(
                        &mut self,
                        field: &tracing::field::Field,
                        value: &dyn std::fmt::Debug,
                    ) {
                        if field.name() == "message" {
                            self.0 = format!("{value:?}");
                        }
                    }
                }
                let mut visitor = Visitor(String::new());
                event.record(&mut visitor);
                self.0.lock().unwrap().push(visitor.0);
            }
        }
    }

    let subscriber = tracing_subscriber::registry().with(CaptureLayer(messages_clone));
    tracing::subscriber::with_default(subscriber, f);
    Arc::try_unwrap(messages).unwrap().into_inner().unwrap()
}

/// `Config::current` requires `Sys: LinkProbe` so the late-stage
/// `store_dir` resolver can probe linkability between project and
/// home. Tests in this
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

/// `Config::current` consults [`GetCurrentDir`] only to anchor a
/// relative `npmrcAuthFile` value. `host_current_dir!(Name)` wires
/// the real-cwd impl onto fakes whose tests never set one.
macro_rules! host_current_dir {
    ($($t:ty),+ $(,)?) => {$(
        impl GetCurrentDir for $t {
            fn current_dir() -> io::Result<PathBuf> {
                std::env::current_dir()
            }
        }
    )+};
}

// Per-test configurable environment fake: env reads come from a fn-local
// `FAKE_ENV`, with no home dir. The state is fn-local, so each `#[test]` gets
// its own environment and concurrent tests never share it. Each test names the
// optional helpers it drives, so every emitted helper is used and none needs a
// `dead_code` allow.
macro_rules! fake_env {
    ($($helper:ident),* $(,)?) => {
        thread_local! {
            static FAKE_ENV: std::cell::RefCell<std::collections::HashMap<String, String>> =
                std::cell::RefCell::new(std::collections::HashMap::new());
            static FAKE_CWD: std::cell::RefCell<Option<PathBuf>> =
                const { std::cell::RefCell::new(None) };
        }

        struct FakeEnv;
        impl EnvVar for FakeEnv {
            fn var(name: &str) -> Option<String> {
                FAKE_ENV.with(|map| map.borrow().get(name).cloned())
            }
            fn vars() -> Vec<(String, String)> {
                FAKE_ENV
                    .with(|map| map.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect())
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
        impl GetCurrentDir for FakeEnv {
            fn current_dir() -> io::Result<PathBuf> {
                FAKE_CWD.with(|cwd| cwd.borrow().clone()).map_or_else(std::env::current_dir, Ok)
            }
        }
        inert_link_probe!(FakeEnv);

        // Reset `FAKE_ENV` to the given env and clear any fake cwd, so a re-run
        // of the same test on the same worker thread starts clean.
        fn set_fake_env(pairs: &[(&str, &str)]) {
            FAKE_ENV.with(|map| {
                let mut map = map.borrow_mut();
                map.clear();
                for (key, value) in pairs {
                    map.insert((*key).to_string(), (*value).to_string());
                }
            });
            FAKE_CWD.with(|cwd| *cwd.borrow_mut() = None);
        }

        $( fake_env!(@helper $helper); )*
    };

    (@helper set_fake_cwd) => {
        fn set_fake_cwd(dir: &Path) {
            FAKE_CWD.with(|cwd| *cwd.borrow_mut() = Some(dir.to_path_buf()));
        }
    };
    (@helper load_with_fake_env) => {
        fn load_with_fake_env(start_dir: &Path) -> Config {
            Config::default().current::<FakeEnv>(start_dir).expect("load config")
        }
    };
    (@helper $unknown:ident) => {
        compile_error!(concat!(
            "unknown `fake_env!` helper `",
            stringify!($unknown),
            "`; expected one of: set_fake_cwd, load_with_fake_env",
        ));
    };
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
host_current_dir!(HostNoHome);

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
    assert_eq!(value.registry, "https://registry.npmjs.org/");
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
pub fn network_settings_defaults_match_pnpm() {
    let value = Config::new();
    assert_eq!(value.network_concurrency, pacquet_network::default_network_concurrency());
    assert_eq!(value.fetch_timeout, 60_000);
    assert!(value.user_agent.starts_with("pnpm/"), "user-agent: {:?}", value.user_agent);
    assert_eq!(value.npmrc_auth_file, None);
}

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

/// Write a `.npmrc` that declares its own registry plus an unscoped
/// `_authToken`, so the token pins to that registry — the shape the
/// precedence assertions check the winning file by.
fn write_registry_auth_file(path: &Path, registry: &str, token: &str) {
    fs::write(path, format!("registry={registry}\n_authToken={token}\n")).expect("write auth file");
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
        config.named_registries.get("work").map(String::as_str),
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
        config.registries.get("default").map(String::as_str),
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
        config.registries.get("@org").map(String::as_str),
        Some("https://npm.pkg.github.com/"),
    );
}

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
        config.registries.get("default").map(String::as_str),
        Some("https://my-npm-proxy.example/"),
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
        config.registries.get("@victim-scope").map(String::as_str),
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
        config.registries.get("@org").map(String::as_str),
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
        config.registries.get("default").map(String::as_str),
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

/// End-to-end: a scoped env JSON entry overrides a repo-controlled
/// `pnpm-workspace.yaml` scoped registry in the main cascade, and the
/// token is pinned to the env-declared host. Asserts
/// `config.registries["@scope"]` — not just auth headers — so a regression
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
        config.registries.get("@victim-scope").map(String::as_str),
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
        config.registries.get("@org").map(String::as_str),
        Some("https://my-npm-proxy.example/"),
    );
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
        Some(pacquet_network::NoProxySetting::List(vec![
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
        Some(pacquet_network::NoProxySetting::List(vec!["internal.example.com".to_string()])),
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

#[test]
pub fn gvs_default_is_off_and_paths_derive_cleanly() {
    let tmp = tempdir().unwrap();
    let config =
        Config::new().current::<HostNoHome>(tmp.path()).expect("workspace yaml absent => no error");
    assert!(
        !config.enable_global_virtual_store,
        "GVS defaults to false (matches pnpm v11 for non-global installs)",
    );
    assert_eq!(config.virtual_store_dir, tmp.path().join("node_modules/.pnpm"));
    assert_eq!(config.global_virtual_store_dir, config.store_dir.links());
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
    assert_eq!(config.virtual_store_dir, tmp.path().join("node_modules/.pnpm"));
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

/// `NPM_CONFIG_WORKSPACE_DIR` must steer `Config::current`'s
/// path-anchoring just like it steers
/// [`pacquet_workspace::find_workspace_dir`] — otherwise the
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
/// workspace-dir value as set. Pairs with `pacquet_workspace`'s
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
    host_current_dir!(HostWithXdgConfigHome);

    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostWithXdgConfigHome>(tmp.path()).expect("config loads");
    assert!(
        config.enable_global_virtual_store,
        "enableGlobalVirtualStore from global config.yaml must apply",
    );
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
    host_current_dir!(HostWithPnpmConfigEnv);

    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostWithPnpmConfigEnv>(tmp.path()).expect("loads");
    assert!(config.enable_global_virtual_store);
}

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
    host_current_dir!(HostWithLowercaseEnv);

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
    host_current_dir!(HostWithPnpmConfigEnv);

    let config = Config::new().current::<HostWithPnpmConfigEnv>(tmp.path()).expect("loads");
    assert!(
        config.enable_global_virtual_store,
        "PNPM_CONFIG_* env var must win over pnpm-workspace.yaml",
    );
}

#[test]
pub fn patches_dir_reads_from_env_overlay() {
    struct HostWithPatchesDirEnv;
    impl EnvVar for HostWithPatchesDirEnv {
        fn var(name: &str) -> Option<String> {
            if name == "PNPM_CONFIG_PATCHES_DIR" {
                return Some("custom-patches".to_owned());
            }
            safe_host_var(name)
        }
    }
    impl EnvVarOs for HostWithPatchesDirEnv {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithPatchesDirEnv {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithPatchesDirEnv);
    host_current_dir!(HostWithPatchesDirEnv);

    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostWithPatchesDirEnv>(tmp.path()).expect("loads");
    assert_eq!(config.patches_dir.as_deref(), Some("custom-patches"));
}

/// `PNPM_CONFIG_HOIST=false` runs the same post-processing as
/// yaml-set `hoist: false` — it short-circuits `hoist_pattern`
/// to `None`, following the rule that `hoist: false` clears the
/// hoist pattern. Without this, the install-time
/// `hoist_pattern.is_some() || public_hoist_pattern.is_some()` guard
/// would still enable hoisting even after the user disabled it via
/// env var.
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
    host_current_dir!(HostWithHoistEnv);

    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostWithHoistEnv>(tmp.path()).expect("loads");
    assert!(!config.hoist);
    assert_eq!(
        config.hoist_pattern, None,
        "hoist: false must clear hoist_pattern, even when set via env var",
    );
}

#[test]
pub fn virtual_store_dir_max_length_matches_pnpm_default() {
    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");
    let expected = if cfg!(windows) { 60 } else { 120 };
    assert_eq!(config.virtual_store_dir_max_length, expected);
}

#[test]
pub fn virtual_store_dir_max_length_from_workspace_yaml() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "virtualStoreDirMaxLength: 90\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert_eq!(config.virtual_store_dir_max_length, 90);
}

#[test]
pub fn engine_strict_node_version_and_max_sockets_default() {
    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");
    assert!(!config.engine_strict);
    assert_eq!(config.node_version, None);
    assert_eq!(config.max_sockets, None);
}

#[test]
pub fn engine_strict_node_version_and_max_sockets_from_workspace_yaml() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "engineStrict: true\nnodeVersion: 18.20.4\nmaxSockets: 5\n",
    )
    .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert!(config.engine_strict);
    assert_eq!(config.node_version.as_deref(), Some("18.20.4"));
    assert_eq!(config.max_sockets, Some(5));
}

#[test]
pub fn cleanup_unused_catalogs_from_workspace_yaml() {
    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");
    assert!(!config.cleanup_unused_catalogs);
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "cleanupUnusedCatalogs: true\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert!(config.cleanup_unused_catalogs);
}

#[test]
pub fn runtime_on_fail_from_workspace_yaml() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("pnpm-workspace.yaml"), "runtimeOnFail: download\n").unwrap();
    let config = Config::default().current::<Host>(dir.path()).unwrap();
    assert_eq!(config.runtime_on_fail, Some(crate::RuntimeOnFail::Download));
}

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
    host_current_dir!(HostWithEnvOverride);

    let config = Config::new().current::<HostWithEnvOverride>(tmp.path()).expect("loads");
    assert_eq!(
        config.virtual_store_dir_max_length, 50,
        "env var must win over pnpm-workspace.yaml",
    );
}

#[test]
pub fn package_map_settings_load_from_workspace_yaml() {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("pnpm-workspace.yaml"),
        "nodeExperimentalPackageMap: true\nnodePackageMapType: loose\n",
    )
    .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert!(config.node_experimental_package_map);
    assert_eq!(config.node_package_map_type, NodePackageMapType::Loose);
}

#[test]
pub fn package_map_settings_load_from_env() {
    struct HostWithPackageMapEnv;
    impl EnvVar for HostWithPackageMapEnv {
        fn var(name: &str) -> Option<String> {
            match name {
                "PNPM_CONFIG_NODE_EXPERIMENTAL_PACKAGE_MAP" => Some("true".to_owned()),
                "PNPM_CONFIG_NODE_PACKAGE_MAP_TYPE" => Some("loose".to_owned()),
                _ => safe_host_var(name),
            }
        }
    }
    impl EnvVarOs for HostWithPackageMapEnv {
        fn var_os(_: &str) -> Option<OsString> {
            None
        }
    }
    impl GetHomeDir for HostWithPackageMapEnv {
        fn home_dir() -> Option<PathBuf> {
            None
        }
    }
    inert_link_probe!(HostWithPackageMapEnv);
    host_current_dir!(HostWithPackageMapEnv);

    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostWithPackageMapEnv>(tmp.path()).expect("loads");
    assert!(config.node_experimental_package_map);
    assert_eq!(config.node_package_map_type, NodePackageMapType::Loose);
}

#[test]
pub fn peers_suffix_max_length_defaults_to_1000() {
    let tmp = tempdir().unwrap();
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("loads");
    assert_eq!(config.peers_suffix_max_length, 1000);
}

#[test]
pub fn peers_suffix_max_length_from_workspace_yaml() {
    let tmp = tempdir().unwrap();
    fs::write(tmp.path().join("pnpm-workspace.yaml"), "peersSuffixMaxLength: 10\n")
        .expect("write to pnpm-workspace.yaml");
    let config = Config::new().current::<HostNoHome>(tmp.path()).expect("yaml is valid");
    assert_eq!(config.peers_suffix_max_length, 10);
}

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
    host_current_dir!(HostWithEnvOverride);

    let config = Config::new().current::<HostWithEnvOverride>(tmp.path()).expect("loads");
    assert_eq!(config.peers_suffix_max_length, 25, "env var must win over pnpm-workspace.yaml");
}

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

const NPM_DEFAULT_REGISTRY: &str = "https://registry.npmjs.org/";

/// A project `.npmrc` redirecting the default registry still drives normal
/// installs, but must NOT steer package-manager bootstrap: that resolves
/// through the trusted user-level registry instead. Regression test for
/// GHSA-j2hc-m6cf-6jm8 (`packageManager` auto-switch registry confusion).
#[test]
pub fn package_manager_bootstrap_ignores_project_npmrc_registry() {
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    write_file(&user_file, "registry=https://trusted.example.com/\n");

    let config = load_with_project_and_user("registry=https://attacker.example.com/\n", user_file);

    assert_eq!(
        config.registry, "https://attacker.example.com/",
        "project registry drives normal installs",
    );
    assert_eq!(
        config.package_manager_bootstrap.registry, "https://trusted.example.com/",
        "package-manager bootstrap ignores the repository-controlled project .npmrc registry",
    );
    assert_eq!(
        config.package_manager_bootstrap.resolved_registries().get("default").map(String::as_str),
        Some("https://trusted.example.com/"),
    );
}

/// A `pnpm-workspace.yaml` `registries:` block is repository-controlled, so
/// it must not steer package-manager bootstrap either — neither the default
/// nor a scoped route. Regression test for GHSA-j2hc-m6cf-6jm8.
#[test]
pub fn package_manager_bootstrap_ignores_workspace_yaml_registries() {
    let auth = tempdir().expect("auth tempdir");
    let user_file = auth.path().join("user-npmrc");
    write_file(&user_file, "registry=https://trusted.example.com/\n");

    let project = tempdir().expect("project tempdir");
    fs::write(
        project.path().join("pnpm-workspace.yaml"),
        "registries:\n  default: https://attacker.example.com/\n  '@evil': https://attacker-scoped.example.com/\n",
    )
    .expect("write pnpm-workspace.yaml");
    let config = Config { npmrc_auth_file: Some(user_file), ..Config::default() }
        .current::<HostNoHome>(project.path())
        .expect("load config");

    assert_eq!(
        config.registry, "https://attacker.example.com/",
        "workspace yaml drives normal installs",
    );
    assert_eq!(
        config.registries.get("@evil").map(String::as_str),
        Some("https://attacker-scoped.example.com/"),
    );
    assert_eq!(
        config.package_manager_bootstrap.registry, "https://trusted.example.com/",
        "package-manager bootstrap ignores the workspace yaml default registry",
    );
    assert_eq!(
        config.package_manager_bootstrap.registries.get("@evil"),
        None,
        "package-manager bootstrap ignores workspace yaml scoped registries",
    );
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

/// `PNPM_CONFIG_REGISTRY` is user-controlled (not repository-controlled), so
/// it overrides the package-manager bootstrap default registry too,
/// mirroring pnpm's env/CLI `registry` handling.
#[test]
pub fn package_manager_bootstrap_honors_env_registry() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    write_file(&project.path().join(".npmrc"), "registry=https://attacker.example.com/\n");
    set_fake_env(&[("PNPM_CONFIG_REGISTRY", "https://env.example.com/")]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.registry, "https://env.example.com/", "env registry drives normal installs");
    assert_eq!(
        config.package_manager_bootstrap.registry, "https://env.example.com/",
        "env registry overrides the package-manager bootstrap default",
    );
}

/// When `PNPM_CONFIG_REGISTRY` is set without a trailing slash, the env
/// override normalizes it before storing — matching pnpm, which treats
/// `https://r` and `https://r/` as the same registry. The slash must be
/// appended consistently to `config.registry`, `config.registries`, and
/// the bootstrap map so downstream lookups (auth-header pinning,
/// `package_manager_bootstrap`) all key against the normalized form.
#[test]
pub fn env_registry_override_appends_missing_trailing_slash() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    set_fake_env(&[("PNPM_CONFIG_REGISTRY", "https://env.example.com")]);

    let config = load_with_fake_env(project.path());

    assert_eq!(config.registry, "https://env.example.com/");
    assert_eq!(
        config.registries.get("default").map(String::as_str),
        Some("https://env.example.com/"),
    );
    assert_eq!(
        config.package_manager_bootstrap.registries.get("default").map(String::as_str),
        Some("https://env.example.com/"),
    );
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

// Port of `extraBinPaths` in `config/reader/test/index.ts` — empty outside
// a workspace; exactly the workspace root's `node_modules/.bin` inside one.
#[test]
pub fn extra_bin_paths_lists_workspace_root_bin_only_inside_a_workspace() {
    fake_env!(load_with_fake_env);
    let project = tempdir().expect("project tempdir");
    set_fake_env(&[]);

    let config = load_with_fake_env(project.path());
    assert_eq!(config.extra_bin_paths, Vec::<PathBuf>::new());

    fs::write(project.path().join("pnpm-workspace.yaml"), "packages:\n  - .\n")
        .expect("write pnpm-workspace.yaml");
    let config = load_with_fake_env(project.path());
    assert_eq!(config.extra_bin_paths, vec![project.path().join("node_modules").join(".bin")]);
}
