use crate::{
    PreparedRegistryInfo, RegistryAnchor, RegistryInfo, pick_port::pick_unused_port, pnpr_command,
    port_to_url::port_to_url,
};
use pipe_trait::Pipe;
use reqwest::Client;
use std::{
    fs::File,
    path::Path,
    process::{Child, Stdio},
};
use tokio::time::{Duration, sleep};

/// Handler of a mocked registry server instance.
///
/// The internal `pnpr` process is terminated on [drop](Drop).
#[derive(Debug)]
pub struct MockInstance {
    pub(crate) process: Child,
}

impl Drop for MockInstance {
    fn drop(&mut self) {
        let MockInstance { process } = self;
        let pid = process.id();
        let _ = process.kill();
        let _ = process.wait();
        eprintln!("info: Terminated pnpr pid {pid}");
    }
}

/// Launch options for a [`MockInstance`].
#[derive(Debug, Clone, Copy)]
pub struct MockInstanceOptions<'a> {
    pub client: &'a Client,
    pub port: u16,
    pub public_url: Option<&'a str>,
    pub stdout: Option<&'a Path>,
    pub stderr: Option<&'a Path>,
    pub max_retries: usize,
    pub retry_delay: Duration,
}

impl MockInstanceOptions<'_> {
    async fn is_registry_ready(self) -> bool {
        let MockInstanceOptions { client, port, .. } = self;
        let url = port_to_url(port);

        let Err(error) = client.head(url).send().await else {
            return true;
        };

        if error.is_connect() {
            eprintln!("info: {error}");
            return false;
        }

        panic!("{error}");
    }

    async fn wait_for_registry(self) {
        let MockInstanceOptions { max_retries, retry_delay, .. } = self;
        let mut retries = max_retries;

        while !self.is_registry_ready().await {
            retries = retries.checked_sub(1).unwrap_or_else(|| {
                panic!("Failed to check for the registry for {max_retries} times")
            });

            sleep(retry_delay).await;
        }
    }

    pub(crate) async fn spawn(self) -> MockInstance {
        let MockInstanceOptions { port, public_url, stdout, stderr, .. } = self;

        let stdout = stdout.map_or_else(Stdio::null, |stdout| {
            File::create(stdout).expect("create file for stdout").into()
        });
        let stderr = stderr.map_or_else(Stdio::null, |stderr| {
            File::create(stderr).expect("create file for stderr").into()
        });
        // Storage is built from the in-repo fixtures (see
        // `registry_mock_storage`) and seeded into runtime storage by
        // `pnpr_command`. pnpr runs in proxy mode
        // against npmjs.org so off-fixture packages fall through to
        // npm; see `pnpr_command` for the rationale.
        let process = pnpr_command(port, public_url)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .expect("spawn pnpr");

        self.wait_for_registry().await;

        MockInstance { process }
    }

    pub async fn spawn_if_necessary(self) -> Option<MockInstance> {
        let MockInstanceOptions { port, .. } = self;
        if self.is_registry_ready().await {
            eprintln!("info: {port} is already available");
            None
        } else {
            eprintln!("info: spawning pnpr...");
            self.spawn().await.pipe(Some)
        }
    }
}

/// Manage a single mocked registry server instance that is shared between multiple different tests.
///
/// This instance can either be automatically be spawned by the first test and tracked by a reference counter
/// or be prepared by the CLI command.
#[derive(Debug)]
#[must_use]
pub enum AutoMockInstance {
    /// The instance is created by the CLI command and managed manually.
    Prepared(PreparedRegistryInfo),
    /// The instance is automatically spawned by the first test to run and managed automatically by counting references.
    RefCount(RegistryAnchor),
}

impl AutoMockInstance {
    pub fn load_or_init() -> Self {
        if let Some(prepared) = PreparedRegistryInfo::try_load() {
            return AutoMockInstance::Prepared(prepared);
        }

        let client = Client::new();
        let anchor = RegistryAnchor::load_or_init(MockInstanceOptions {
            client: &client,
            port: pick_unused_port().expect("pick an unused port"),
            public_url: None,
            stdout: None,
            stderr: None,
            max_retries: 20,
            retry_delay: Duration::from_millis(500),
        });

        AutoMockInstance::RefCount(anchor)
    }

    fn info(&self) -> &'_ RegistryInfo {
        match self {
            AutoMockInstance::Prepared(prepared) => &prepared.info,
            AutoMockInstance::RefCount(anchor) => &anchor.info,
        }
    }

    #[must_use]
    pub fn url(&self) -> String {
        self.info().url()
    }
}
