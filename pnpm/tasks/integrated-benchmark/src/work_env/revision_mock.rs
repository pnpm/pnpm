//! How the benchmark launches a revision's mocks and pnpr server.

use super::{
    PnprServerPaths, RevisionMockRegistry, registries_home::RegistriesHome,
    server_config::pnpr_benchmark_public_url,
};
use crate::latency_proxy::LatencyProxy;
use std::{
    fs::File,
    path::Path,
    process::{Child, Command, Stdio},
};

/// The origin a revision's mock advertises in its packuments. A cold mock
/// whose registries live in the database may not proxy its warm origin on
/// its own host, so it advertises `localhost` while the origin is reached at
/// `127.0.0.1`.
pub(super) fn served_revision_mock_origin(
    registry: &RevisionMockRegistry,
    cold: bool,
    registries: RegistriesHome,
) -> String {
    if !cold || registries == RegistriesHome::ConfigFile {
        return registry.origin.clone();
    }
    let port = registry.listener
        .local_addr()
        .expect("revision mock proxy local addr")
        .port();
    format!("http://localhost:{port}")
}

pub(super) fn spawn_revision_mock(mut command: Command, bench_dir: &Path, cold: bool) -> Child {
    let stdout = File::create(bench_dir.join("revision-mock.stdout.log"))
        .expect("create revision mock stdout log");
    let stderr = File::create(bench_dir.join("revision-mock.stderr.log"))
        .expect("create revision mock stderr log");
    command
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .expect(if cold { "spawn cold revision mock" } else { "spawn revision mock" })
}

/// The command that serves one benchmark's pnpr server. A server whose
/// registries live in the database gets [`pnpr_benchmark_public_url`].
pub(super) fn pnpr_server_command(paths: &PnprServerPaths<'_>) -> Command {
    let mut command = Command::new(paths.binary);
    command
        .arg("--config")
        .arg(paths.config)
        .arg("--listen")
        .arg(format!("127.0.0.1:{}", paths.port))
        .arg("--storage")
        .arg(paths.storage)
        // The resolver resolves against the registry the client
        // sends, caching packuments in its own store. A long TTL keeps
        // those cached packuments authoritative across the run, the
        // same value the registry-mock pins for the same reason.
        .arg("--packument-ttl")
        .arg("31536000");
    if paths.registries.is_some() {
        command
            .arg("--public-url")
            .arg(pnpr_benchmark_public_url(paths.port));
    }
    command
}

/// The `--public-url` of a cold mock's warm origin: the latency proxy in
/// front of it, if any. The cold mock fetches the tarball URLs it
/// advertises, and one whose registries live in the database may not reach
/// its own `localhost` host, so the origin then names its address.
pub(super) fn cold_origin_public_url(
    latency_proxy: Option<&LatencyProxy>,
    registries: RegistriesHome,
    origin_port: u16,
) -> Option<String> {
    latency_proxy
        .map(|proxy| format!("http://{}", proxy.addr))
        .or_else(|| {
            (registries == RegistriesHome::Database).then(|| {
                format!("http://127.0.0.1:{origin_port}")
            })
        })
}
