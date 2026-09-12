use serde_json::Value;
use std::{net::TcpListener as StdTcpListener, path::Path, process::Output, time::Duration};
use tempfile::TempDir;
use tokio::{net::TcpListener, process::Command};

use super::{app, config};

struct CompilerSession {
    directory: TempDir,
    port: u16,
    endpoint: String,
    readonly: bool,
}

impl CompilerSession {
    async fn start(endpoint: String, readonly: bool) -> Self {
        let directory = TempDir::new().unwrap();
        std::fs::write(directory.path().join("sccache.toml"), "").unwrap();
        let listener = StdTcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let session = Self { directory, port, endpoint, readonly };
        successful(session.command("sccache").arg("--start-server").output().await.unwrap());
        session
    }

    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("SCCACHE_") {
                command.env_remove(name);
            }
        }
        command
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("CARGO_MAKEFLAGS")
            .env_remove("RUSTC_WORKSPACE_WRAPPER")
            .env("RUSTFLAGS", "")
            .env("RUSTC_WRAPPER", "sccache")
            .env("CARGO_INCREMENTAL", "0")
            .env("CARGO_PROFILE_DEV_DEBUG", "0")
            .env("SCCACHE_CONF", self.directory.path().join("sccache.toml"))
            .env("SCCACHE_CACHED_CONF", self.directory.path().join("cached-config"))
            .env("SCCACHE_DIR", self.directory.path().join("cache"))
            .env("SCCACHE_SERVER_PORT", self.port.to_string())
            .env("SCCACHE_IDLE_TIMEOUT", "60")
            .env("SCCACHE_LOG", "debug")
            .env("SCCACHE_ERROR_LOG", self.directory.path().join("sccache.log"))
            .env("SCCACHE_MULTILEVEL_CHAIN", "disk,webdav")
            .env("SCCACHE_MULTILEVEL_WRITE_ERROR_POLICY", "all")
            .env("SCCACHE_WEBDAV_ENDPOINT", &self.endpoint)
            .env("SCCACHE_WEBDAV_TOKEN", "token")
            .env("SCCACHE_WEBDAV_RW_MODE", if self.readonly { "READ_ONLY" } else { "READ_WRITE" });
        command
    }

    async fn build(&self, project: &Path, extra: &[&str]) {
        successful(
            self.command("cargo")
                .current_dir(project)
                .env("CARGO_TARGET_DIR", project.join("target"))
                .args(["build", "--offline"])
                .args(extra)
                .output()
                .await
                .unwrap(),
        );
        assert!(project.join("target/debug/libcache_fixture.rlib").is_file(), "missing rlib");
    }

    async fn stats(&self) -> Value {
        let output = successful(
            self.command("sccache")
                .args(["--show-stats", "--stats-format", "json"])
                .output()
                .await
                .unwrap(),
        );
        let stats: Value = serde_json::from_slice(&output.stdout).unwrap();
        if stats["stats"]["cache_write_errors"] != 0 {
            eprintln!(
                "{}",
                std::fs::read_to_string(self.directory.path().join("sccache.log")).unwrap(),
            );
        }
        stats
    }
}

impl Drop for CompilerSession {
    fn drop(&mut self) {
        if let Err(error) = self.command("sccache").into_std().arg("--stop-server").output() {
            eprintln!("could not stop test sccache server: {error}");
        }
    }
}

fn successful(output: Output) -> Output {
    assert!(output.status.success(), "command failed: {output:?}");
    output
}

fn project(directory: &Path) {
    std::fs::create_dir_all(directory.join("src")).unwrap();
    std::fs::write(directory.join("Cargo.toml"), "[package]\nname = 'cache-fixture'\nversion = '0.0.0'\nedition = '2024'\n[features]\nextra = []\n[workspace]\n").unwrap();
    std::fs::write(directory.join("src/lib.rs"), "pub fn answer() -> u32 { 42 }\n").unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn cargo_reuses_ci_compilation_with_fresh_checkout_and_backfills_disk() {
    let directory = TempDir::new().unwrap();
    let config = config(&directory);
    let ci_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ci_endpoint =
        format!("http://{}/-/pnpr/v0/compiler-cache/acme", ci_listener.local_addr().unwrap());
    let ci_app = app(config.clone(), "ci", false);
    let ci_server = tokio::spawn(async move { axum::serve(ci_listener, ci_app).await.unwrap() });
    let ci_project = directory.path().join("checkout");
    project(&ci_project);
    let ci = CompilerSession::start(ci_endpoint, false).await;
    ci.build(&ci_project, &[]).await;
    let stats = ci.stats().await;
    assert_eq!(stats["stats"]["cache_misses"]["counts"]["Rust"], 1, "{stats}");
    assert_eq!(stats["stats"]["cache_write_errors"], 0, "{stats}");
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let stats = ci.stats().await;
            assert_eq!(stats["stats"]["cache_write_errors"], 0, "{stats}");
            if stats["stats"]["cache_writes"] == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("CI compilation must finish uploading before stopping pnpr");
    drop(ci);
    ci_server.abort();
    // Rust cache keys include the absolute working directory. Separate machines
    // need a matching mount path; this fresh checkout models that arrangement.
    std::fs::rename(&ci_project, directory.path().join("ci-checkout")).unwrap();

    let dev_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dev_endpoint =
        format!("http://{}/-/pnpr/v0/compiler-cache/acme", dev_listener.local_addr().unwrap());
    let dev_app = app(config, "developer", false);
    let dev_server = tokio::spawn(async move { axum::serve(dev_listener, dev_app).await.unwrap() });
    let dev_project = directory.path().join("checkout");
    project(&dev_project);
    let developer = CompilerSession::start(dev_endpoint, true).await;
    developer.build(&dev_project, &[]).await;
    let stats = developer.stats().await;
    assert_eq!(stats["stats"]["cache_hits"]["counts"]["Rust"], 1, "{stats}");
    developer.build(&dev_project, &["--features", "extra"]).await;
    let stats = developer.stats().await;
    assert_eq!(stats["stats"]["cache_misses"]["counts"]["Rust"], 1, "{stats}");
    std::fs::write(dev_project.join("src/lib.rs"), "pub fn answer() -> u32 { 43 }\n").unwrap();
    developer.build(&dev_project, &[]).await;
    let stats = developer.stats().await;
    assert_eq!(stats["stats"]["cache_misses"]["counts"]["Rust"], 2, "{stats}");
    std::fs::write(dev_project.join("src/lib.rs"), "pub fn answer() -> u32 { 42 }\n").unwrap();
    dev_server.abort();
    successful(
        developer
            .command("cargo")
            .current_dir(&dev_project)
            .env("CARGO_TARGET_DIR", dev_project.join("target"))
            .arg("clean")
            .output()
            .await
            .unwrap(),
    );
    developer.build(&dev_project, &[]).await;
    let stats = developer.stats().await;
    assert_eq!(stats["stats"]["cache_hits"]["counts"]["Rust"], 2, "{stats}");
    assert_eq!(stats["stats"]["cache_read_errors"], 0, "{stats}");
}
