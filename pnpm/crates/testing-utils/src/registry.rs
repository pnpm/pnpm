use pnpr::{Config, Ecosystem};
use pnpr_fixtures::MockConfigPresets;
use std::{
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex, PoisonError},
    thread,
};

#[derive(Debug)]
#[must_use]
pub struct TestRegistry {
    server_url: String,
    url: String,
    /// The fixture storage this registry serves, for the tests that own it
    /// outright. [`None`] for the process-global instance, whose storage
    /// every other test reads.
    storage: Option<PathBuf>,
    /// The requests answered so far, for a registry started with
    /// [`Self::start_recording_with_own_storage`].
    requests: Option<RequestLog>,
}

/// One request the registry answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedRequest {
    pub method: String,
    pub path: String,
    pub accept: Option<String>,
    pub if_none_match: Option<String>,
    pub if_modified_since: Option<String>,
    pub status: u16,
    /// The response's `ETag`.
    pub etag: Option<String>,
}

type RequestLog = Arc<Mutex<Vec<RecordedRequest>>>;

impl TestRegistry {
    pub fn start() -> Self {
        Self::new(TestRegistryInstance::get().server_url.clone(), None, None)
    }

    pub fn start_with_storage(storage: &Path) -> Self {
        let instance = TestRegistryInstance::start(storage.to_path_buf(), RegistryMode::Proxy);
        Self::new(instance.server_url, Some(storage.to_path_buf()), None)
    }

    /// Like [`Self::start_with_own_storage`], and records every request it
    /// answers for [`Self::take_requests`].
    pub fn start_recording_with_own_storage(root: &Path) -> Self {
        let storage = root.join("registry-storage");
        pnpr_fixtures::build_storage_at_with_substitutions(
            &pnpr_fixtures::packages_dir(),
            &storage,
            &[],
        );
        let requests = RequestLog::default();
        let instance = TestRegistryInstance::start_with_log(
            storage.clone(),
            RegistryMode::Proxy,
            Some(Arc::clone(&requests)),
        );
        Self::new(instance.server_url, Some(storage), Some(requests))
    }

    /// The requests answered since the last call, in the order they finished.
    pub fn take_requests(&self) -> Vec<RecordedRequest> {
        let requests = self.requests
            .as_ref()
            .expect(
                "only a registry started with start_recording_with_own_storage records requests",
            );
        requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .collect()
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
        let instance = TestRegistryInstance::start(storage.to_path_buf(), RegistryMode::Static);
        Self::new(instance.server_url, Some(storage.to_path_buf()), None)
    }

    fn new(server_url: String, storage: Option<PathBuf>, requests: Option<RequestLog>) -> Self {
        let url = format!("{}{}/", server_url.trim_end_matches('/'), Ecosystem::Npm.base_path());
        Self { server_url, url, storage, requests }
    }

    /// The npm registry URL, with a trailing `/`.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The server's own URL, with a trailing `/`, where the pnpr protocols
    /// answer.
    #[must_use]
    pub fn server_url(&self) -> &str {
        &self.server_url
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

    /// See [`pnpr_fixtures::remove_version_field`]. Needs a registry of the
    /// test's own, as [`Self::set_dist_tag`] does.
    pub fn remove_version_field(&self, package: &str, version: &str, field: &str) {
        let storage = self.storage.as_deref().expect(
            "editing metadata needs a registry of the test's own — start it with add_mocked_registry_with_own_storage",
        );
        pnpr_fixtures::remove_version_field(storage, package, version, field);
    }
}

#[derive(Debug)]
struct TestRegistryInstance {
    server_url: String,
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
        Self::start_with_log(storage, mode, None)
    }

    fn start_with_log(storage: PathBuf, mode: RegistryMode, requests: Option<RequestLog>) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .expect("bind test registry to an unused localhost port");
        listener.set_nonblocking(true).expect("set test registry listener to nonblocking");
        let listen = listener.local_addr().expect("read test registry listener address");

        let server_url = format!("http://{listen}/");
        // Proxy mode: `@pnpm.e2e` fixtures are served from local storage, while
        // real npm packages (`is-positive`, `is-negative`, etc.) fall through to
        // the npm upstream — matching how registry-mock served pacquet's tests.
        let mut config = match mode {
            RegistryMode::Proxy => {
                let mut config = Config::proxy(listen, storage);
                pnpr_fixtures::with_tar_registry(&mut config);
                config
            }
            RegistryMode::Static => Config::static_serve(listen, storage),
        };
        config.http.public_url = server_url.trim_end_matches('/').to_string();
        // Registration is opt-in; tests that forward credentials create
        // accounts via adduser against this registry.
        config.identity.auth.htpasswd.max_users = pnpr::MaxUsers::Unlimited;
        // A long TTL keeps the fixture packuments (whose `time` values are static)
        // from being treated as stale and refetched from the upstream.
        config.http.packument_ttl = std::time::Duration::from_hours(8760);
        thread::Builder::new()
            .name("pacquet-test-registry".to_string())
            .spawn(move || run_registry(config, listener, requests))
            .expect("spawn test registry thread");

        Self { server_url }
    }
}

fn run_registry(config: Config, listener: TcpListener, requests: Option<RequestLog>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("create test registry runtime");

    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::from_std(listener).expect("create tokio listener");
        let Some(requests) = requests else {
            pnpr::serve_listener(config, listener).await.expect("serve test registry");
            return;
        };
        let app = pnpr::router(config)
            .layer(axum::middleware::from_fn_with_state(requests, record_request));
        axum::serve(listener, app).await.expect("serve test registry");
    });
}

async fn record_request(
    axum::extract::State(requests): axum::extract::State<RequestLog>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let headers = request.headers();
    let accept = header_text(headers, &axum::http::header::ACCEPT);
    let if_none_match = header_text(headers, &axum::http::header::IF_NONE_MATCH);
    let if_modified_since = header_text(headers, &axum::http::header::IF_MODIFIED_SINCE);
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let response = next.run(request).await;
    let status = response.status().as_u16();
    let etag = header_text(response.headers(), &axum::http::header::ETAG);
    requests
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(RecordedRequest {
            method,
            path,
            accept,
            if_none_match,
            if_modified_since,
            status,
            etag,
        });
    response
}

fn header_text(headers: &axum::http::HeaderMap, name: &axum::http::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}
