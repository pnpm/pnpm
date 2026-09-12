use std::{
    env,
    ffi::OsString,
    fmt::Write as _,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use pnpm_store_dir::StoreDir;
use pnpm_testing_utils::env_guard::EnvGuard;
use pretty_assertions::assert_eq;
use tempfile::tempdir;
use tracing::Level;
use tracing_subscriber::{Layer, layer::SubscriberExt};

use super::{
    Config, EnvVar, EnvVarOs, GetCurrentDir, GetHomeDir, Host, LinkProbe, LoadWorkspaceYamlError,
    NodeLinker, NodePackageMapType, PackageImportMethod, TrustPolicy, WorkspaceSettings, fs,
    settings::default_ci,
};
use crate::defaults::{GLOBAL_LAYOUT_VERSION, default_state_dir, default_store_dir};

/// Capture all tracing WARN messages emitted during a closure, each followed
/// by its structured fields, since some warnings say which setting or variable
/// they are about in a field rather than in the message.
pub(crate) fn capture_warnings<Func: FnOnce()>(f: Func) -> Vec<String> {
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
                #[derive(Default)]
                struct Visitor {
                    message: String,
                    fields: String,
                }
                impl Visitor {
                    fn record(&mut self, name: &str, value: String) {
                        if name == "message" {
                            self.message = value;
                        } else {
                            let _ = write!(self.fields, " {name}={value}");
                        }
                    }
                }
                impl tracing::field::Visit for Visitor {
                    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                        self.record(field.name(), value.to_string());
                    }
                    fn record_debug(
                        &mut self,
                        field: &tracing::field::Field,
                        value: &dyn std::fmt::Debug,
                    ) {
                        self.record(field.name(), format!("{value:?}"));
                    }
                }
                let mut visitor = Visitor::default();
                event.record(&mut visitor);
                self.0.lock().unwrap().push(format!("{}{}", visitor.message, visitor.fields));
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

/// Write a `.npmrc` that declares its own registry plus an unscoped
/// `_authToken`, so the token pins to that registry — the shape the
/// precedence assertions check the winning file by.
fn write_registry_auth_file(path: &Path, registry: &str, token: &str) {
    fs::write(path, format!("registry={registry}\n_authToken={token}\n")).expect("write auth file");
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

/// Load a config from a workspace whose `pnpm-workspace.yaml` holds `yaml`.
fn config_from_workspace_yaml(yaml: &str) -> Config {
    let tmp = tempdir().expect("workspace tempdir");
    fs::write(tmp.path().join("pnpm-workspace.yaml"), yaml).expect("write to pnpm-workspace.yaml");
    Config::new().current::<HostNoHome>(tmp.path()).expect("config loads")
}

const NPM_DEFAULT_REGISTRY: &str = "https://registry.npmjs.org/";

/// A scratch git repository whose branch the per-branch lockfile
/// derivation reads: a `.git/HEAD` is all `get_current_branch` needs, so
/// the fixture is a directory rather than a real `git init`.
fn repo_on_branch(head: &str) -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join(".git/HEAD"), head).unwrap();
    dir
}

/// The derivation reads the branch from the process's working directory,
/// so a test that wants a specific branch has to answer
/// [`GetCurrentDir`] with its own fixture rather than the real cwd.
macro_rules! host_in_repo {
    ($name:ident) => {
        struct $name;
        impl EnvVar for $name {
            fn var(name: &str) -> Option<String> {
                safe_host_var(name)
            }
        }
        impl EnvVarOs for $name {
            fn var_os(_: &str) -> Option<OsString> {
                None
            }
        }
        impl GetHomeDir for $name {
            fn home_dir() -> Option<PathBuf> {
                None
            }
        }
        inert_link_probe!($name);
        impl GetCurrentDir for $name {
            fn current_dir() -> io::Result<PathBuf> {
                REPO_DIR.get().cloned().ok_or_else(|| io::Error::other("no repo fixture"))
            }
        }
    };
}

/// Writing a global `config.yaml` whose `_auth` credits `registry` and the
/// project manifest `project_yaml`, then loading config from that project.
fn load_with_auth_file(auth_yaml: &str, project_yaml: Option<&str>) -> Config {
    fake_env!(load_with_fake_env);
    let xdg = tempdir().expect("xdg tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(config_dir.join("config.yaml"), auth_yaml).expect("write global config.yaml");

    let project = tempdir().expect("project tempdir");
    if let Some(project_yaml) = project_yaml {
        fs::write(project.path().join("pnpm-workspace.yaml"), project_yaml)
            .expect("write pnpm-workspace.yaml");
    }
    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);

    load_with_fake_env(project.path())
}

const STORED_LOGIN: &str = "\
_auth:
  https://private.example/:
    '@': { authToken: stored-token }
    '@org': { authToken: stored-org-token }
";

fn load_with_auth_file_and_npmrc(auth_yaml: &str, npmrc: &str) -> Config {
    fake_env!(load_with_fake_env);
    let xdg = tempdir().expect("xdg tempdir");
    let config_dir = xdg.path().join("pnpm");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(config_dir.join("config.yaml"), auth_yaml).expect("write global config.yaml");

    let project = tempdir().expect("project tempdir");
    fs::write(project.path().join(".npmrc"), npmrc).expect("write .npmrc");
    set_fake_env(&[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())]);

    load_with_fake_env(project.path())
}

mod behavior;

mod lockfile;

mod configuration_state_dir_uses_only;

mod configuration_explicit_settings_report_the;

mod authorization_npmrc_auth_file_override;

mod authorization_the_global_auth_file;

mod workspace_settings_json_env_env_default;

mod workspace_settings_materialization_env_vars_override;

mod files;

mod security;

mod reporting;

mod dependencies;

mod integrity;
