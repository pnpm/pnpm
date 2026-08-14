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
}

impl TestRegistry {
    pub fn start() -> Self {
        Self { url: TestRegistryInstance::get().url.clone() }
    }

    pub fn start_with_storage(storage: &Path) -> Self {
        Self { url: TestRegistryInstance::start(storage.to_path_buf(), RegistryMode::Proxy).url }
    }

    pub fn start_static_with_storage(storage: &Path) -> Self {
        Self { url: TestRegistryInstance::start(storage.to_path_buf(), RegistryMode::Static).url }
    }

    #[must_use]
    pub fn url(&self) -> String {
        self.url.clone()
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
