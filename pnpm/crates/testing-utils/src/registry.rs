use pnpr::Config;
use std::{
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    sync::LazyLock,
    thread,
};

#[derive(Debug)]
#[must_use]
pub struct TestRegistry {
    url: String,
    /// The fixture storage this registry serves, for the tests that own it
    /// outright. [`None`] for the process-global instance, whose storage
    /// every other test reads.
    storage: Option<PathBuf>,
}

impl TestRegistry {
    pub fn start() -> Self {
        Self { url: TestRegistryInstance::get().url.clone(), storage: None }
    }

    pub fn start_with_storage(storage: &Path) -> Self {
        Self {
            url: TestRegistryInstance::start(storage.to_path_buf(), RegistryMode::Proxy).url,
            storage: Some(storage.to_path_buf()),
        }
    }

    /// Build fixture storage under `root` and start a registry over it.
    /// The tree is the caller's alone, so it may be re-tagged with
    /// [`Self::set_dist_tag`].
    pub fn start_with_own_storage(root: &Path) -> Self {
        Self::start_over_built_storage(root, &[], false)
    }

    pub(crate) fn start_over_built_storage(
        root: &Path,
        substitutions: &[(&str, &str)],
        static_serve: bool,
    ) -> Self {
        let storage = root.join("registry-storage");
        pnpr_fixtures::build_storage_at_with_substitutions(
            &pnpr_fixtures::packages_dir(),
            &storage,
            substitutions,
        );
        if static_serve {
            Self::start_static_with_storage(&storage)
        } else {
            Self::start_with_storage(&storage)
        }
    }

    pub fn start_static_with_storage(storage: &Path) -> Self {
        Self {
            url: TestRegistryInstance::start(storage.to_path_buf(), RegistryMode::Static).url,
            storage: Some(storage.to_path_buf()),
        }
    }

    #[must_use]
    pub fn url(&self) -> String {
        self.url.clone()
    }

    /// Move `tag` to `version`, the way the JS harness's `addDistTag` does.
    /// A fixture package's `latest` is otherwise its highest published
    /// version, which cannot express "the newer version was published after
    /// the install" — the setup every upstream `--latest` test relies on.
    ///
    /// Only a registry started over storage of the test's own can be
    /// re-tagged — see `CommandTempCwd::add_mocked_registry_with_own_storage`;
    /// the process-global instance is shared.
    pub fn set_dist_tag(&self, package: &str, version: &str, tag: &str) {
        let storage = self.storage.as_deref().expect(
            "re-tagging needs a registry of the test's own — start it with add_mocked_registry_with_own_storage",
        );
        pnpr_fixtures::set_dist_tag(storage, package, version, tag);
    }
}

#[derive(Debug)]
struct TestRegistryInstance {
    url: String,
}

#[derive(Clone, Copy)]
enum RegistryMode {
    Proxy,
    Static,
}

impl TestRegistryInstance {
    fn get() -> &'static Self {
        static INSTANCE: LazyLock<TestRegistryInstance> = LazyLock::new(|| {
            TestRegistryInstance::start(
                pnpr_fixtures::ensure_storage().to_path_buf(),
                RegistryMode::Proxy,
            )
        });
        &INSTANCE
    }

    fn start(storage: PathBuf, mode: RegistryMode) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("bind test registry to an unused localhost port");
        listener.set_nonblocking(true).expect("set test registry listener to nonblocking");
        let listen = listener.local_addr().expect("read test registry listener address");

        let url = format!("http://{listen}/");
        // Proxy mode: `@pnpm.e2e` fixtures are served from local storage, while
        // real npm packages (`is-positive`, `is-negative`, etc.) fall through to
        // the npm upstream — matching how registry-mock served pacquet's tests.
        let mut config = match mode {
            RegistryMode::Proxy => Config::proxy(listen, storage),
            RegistryMode::Static => Config::static_serve(listen, storage),
        };
        config.public_url = url.trim_end_matches('/').to_string();
        // Registration is opt-in; tests that forward credentials create
        // accounts via adduser against this registry.
        config.auth.htpasswd.max_users = pnpr::MaxUsers::Unlimited;
        // A long TTL keeps the fixture packuments (whose `time` values are static)
        // from being treated as stale and refetched from the upstream.
        config.packument_ttl = std::time::Duration::from_hours(8760);
        thread::Builder::new()
            .name("pacquet-test-registry".to_string())
            .spawn(move || run_registry(config, listener))
            .expect("spawn test registry thread");

        Self { url }
    }
}

fn run_registry(config: Config, listener: TcpListener) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("create test registry runtime");

    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::from_std(listener).expect("create tokio listener");
        pnpr::serve_listener(config, listener).await.expect("serve test registry");
    });
}
