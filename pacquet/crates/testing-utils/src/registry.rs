use pnpr::Config;
use std::{
    net::{Ipv4Addr, TcpListener},
    sync::OnceLock,
    thread,
};

#[derive(Debug)]
#[must_use]
pub struct TestRegistry {
    instance: &'static TestRegistryInstance,
}

impl TestRegistry {
    pub fn start() -> Self {
        Self { instance: TestRegistryInstance::get() }
    }

    #[must_use]
    pub fn url(&self) -> String {
        self.instance.url.clone()
    }
}

#[derive(Debug)]
struct TestRegistryInstance {
    url: String,
}

impl TestRegistryInstance {
    fn get() -> &'static Self {
        static INSTANCE: OnceLock<TestRegistryInstance> = OnceLock::new();
        INSTANCE.get_or_init(Self::start)
    }

    fn start() -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("bind test registry to an unused localhost port");
        listener.set_nonblocking(true).expect("set test registry listener to nonblocking");
        let listen = listener.local_addr().expect("read test registry listener address");

        let url = format!("http://{listen}/");
        let storage = pnpr_fixtures::ensure_storage();
        // Proxy mode: `@pnpm.e2e` fixtures are served from local storage, while
        // real npm packages (`is-positive`, `is-negative`, etc.) fall through to
        // the npm uplink — matching how registry-mock served pacquet's tests.
        let mut config = Config::proxy(listen, storage.to_path_buf());
        config.public_url = url.trim_end_matches('/').to_string();
        // A long TTL keeps the fixture packuments (whose `time` values are static)
        // from being treated as stale and refetched from the uplink.
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
