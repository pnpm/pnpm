use crate::{
    PreparedRegistryInfo, RegistryAnchor, RegistryInfo,
    kill_verdaccio::kill_all_verdaccio_children, node_registry_mock, pick_port::pick_unused_port,
    port_to_url::port_to_url,
};
use assert_cmd::prelude::*;
use pipe_trait::Pipe;
use reqwest::Client;
use std::{
    fs::File,
    path::Path,
    process::{Child, Stdio},
};
use sysinfo::{Pid, Signal};
use tokio::time::{Duration, sleep};

/// Handler of a mocked registry server instance.
///
/// The internal process will be killed on [drop](Drop).
#[derive(Debug)]
pub struct MockInstance {
    pub(crate) process: Child,
}

impl Drop for MockInstance {
    fn drop(&mut self) {
        let MockInstance { process, .. } = self;
        let pid = process.id();
        eprintln!("info: Terminating all verdaccio instances below {pid}...");
        let kill_count = kill_all_verdaccio_children(Pid::from_u32(pid), Signal::Interrupt);
        eprintln!("info: Terminated {kill_count} verdaccio instances");
    }
}

/// Launch options for a [`MockInstance`].
#[derive(Debug, Clone, Copy)]
pub struct MockInstanceOptions<'a> {
    pub client: &'a Client,
    pub port: u16,
    pub stdout: Option<&'a Path>,
    pub stderr: Option<&'a Path>,
    pub max_retries: usize,
    pub retry_delay: Duration,
}

impl<'a> MockInstanceOptions<'a> {
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
        let MockInstanceOptions { port, stdout, stderr, .. } = self;
        let port = port.to_string();

        eprintln!("Preparing...");
        node_registry_mock()
            .arg("prepare")
            .env("PNPM_REGISTRY_MOCK_PORT", &port)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .assert()
            .success();

        let stdout = stdout.map_or_else(Stdio::null, |stdout| {
            File::create(stdout).expect("create file for stdout").into()
        });
        let stderr = stderr.map_or_else(Stdio::null, |stderr| {
            File::create(stderr).expect("create file for stderr").into()
        });
        let process = node_registry_mock()
            .env("PNPM_REGISTRY_MOCK_PORT", &port)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .expect("spawn mocked registry");

        self.wait_for_registry().await;

        MockInstance { process }
    }

    pub async fn spawn_if_necessary(self) -> Option<MockInstance> {
        let MockInstanceOptions { port, .. } = self;
        if self.is_registry_ready().await {
            eprintln!("info: {port} is already available");
            None
        } else {
            eprintln!("info: spawning mocked registry...");
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

    pub fn url(&self) -> String {
        self.info().url()
    }
}
