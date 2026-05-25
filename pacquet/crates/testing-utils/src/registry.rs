use pnpm_registry::Config;
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
        let storage = pnpm_registry_fixtures::ensure_storage();
        let mut config = Config::static_serve(listen, storage.to_path_buf());
        config.public_url = url.trim_end_matches('/').to_string();
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
        pnpm_registry::serve_listener(config, listener).await.expect("serve test registry");
    });
}
