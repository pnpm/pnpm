mod hooks;

mod store;

mod runtimes;

mod links;

mod workspace;

mod integrity;

mod resolution;

mod installation;

mod reporting;

use super::{InstallPackageBySnapshotError, host_platform_selector};
use pnpm_config::Config;
use pnpm_lockfile::{LockfileResolution, PackageKey};

/// A dummy but parseable sha512 integrity for the registry-resolution
/// fixtures below. The download never runs in these tests (the mem
/// cache short-circuits, or `offline` blocks), so the exact digest is
/// irrelevant — it only has to satisfy [`ssri::Integrity`]'s parser.
const DUMMY_SHA512: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

fn registry_metadata() -> pnpm_lockfile::PackageMetadata {
    pnpm_lockfile::PackageMetadata {
        resolution: LockfileResolution::Registry(pnpm_lockfile::RegistryResolution {
            integrity: DUMMY_SHA512.parse().expect("parse integrity"),
            revision: None,
        }),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

fn leaked_offline_config(
    registry: &str,
    store_dir: &std::path::Path,
) -> &'static pnpm_config::Config {
    let mut config = pnpm_config::Config::new();
    config.registry = registry.to_string();
    config.store_dir = store_dir.to_path_buf().into();
    // Force the no-mem-cache download path to fail fast instead of
    // reaching out to the network, so a regression that bypasses the
    // mem cache surfaces deterministically as `NoOfflineTarball`.
    config.offline = true;
    config.leak()
}

/// A test-double custom fetcher: claims (or declines) every package and
/// answers `fetch` with one scripted envelope, so each test below pins
/// one branch of the delegate handling in
/// [`super::InstallPackageBySnapshot::run`].
struct ScriptedCustomFetcher {
    claims: bool,
    response: Result<serde_json::Value, pnpm_hooks::HookError>,
}

#[async_trait::async_trait]
impl pnpm_hooks::CustomFetcher for ScriptedCustomFetcher {
    async fn can_fetch(
        &self,
        _pkg_id: &str,
        _resolution: serde_json::Value,
    ) -> Result<bool, pnpm_hooks::HookError> {
        Ok(self.claims)
    }

    async fn fetch(
        &self,
        _pkg_id: &str,
        _resolution: serde_json::Value,
        _opts: serde_json::Value,
    ) -> Result<serde_json::Value, pnpm_hooks::HookError> {
        self.response.clone()
    }
}

fn scripted_session(
    claims: bool,
    response: Result<serde_json::Value, pnpm_hooks::HookError>,
) -> std::sync::Arc<crate::CustomFetcherSession> {
    std::sync::Arc::new(crate::CustomFetcherSession::new(vec![std::sync::Arc::new(
        ScriptedCustomFetcher { claims, response },
    )]))
}

/// Run one snapshot install for `foo@1.0.0` with the given custom
/// fetcher session. Hoisted linker + no store index keeps the run to the
/// fetch-dispatch branch under test, mirroring the mem-cache tests
/// above.
async fn run_snapshot_install_with_session(
    config: &'static Config,
    metadata: &pnpm_lockfile::PackageMetadata,
    session: &std::sync::Arc<crate::CustomFetcherSession>,
    tarball_mem_cache: Option<&std::sync::Arc<pnpm_tarball::MemCache>>,
    workspace_root: &std::path::Path,
) -> Result<super::InstalledPackage, InstallPackageBySnapshotError> {
    let package_key: PackageKey = "foo@1.0.0".parse().expect("parse key");
    let layout = crate::VirtualStoreLayout::legacy(workspace_root.join("vstore"), 120);
    let allow_build_policy = crate::AllowBuildPolicy::new(
        std::collections::HashSet::default(),
        std::collections::HashSet::default(),
        false,
    );
    let skipped = crate::SkippedSnapshots::new();
    let logged_methods = std::sync::atomic::AtomicU8::new(0);
    let verified_files_cache = pnpm_store_dir::SharedVerifiedFilesCache::default();
    let snapshot = pnpm_lockfile::SnapshotEntry::default();

    super::InstallPackageBySnapshot {
        ctx: &crate::InstallContext {
            config,
            workspace_root,
            requester: "/project",
            layout: &layout,
            node_linker: pnpm_config::NodeLinker::Hoisted,
            allow_build_policy: &allow_build_policy,
            link_options: &pnpm_cmd_shim::LinkBinsOptions::default(),
            logged_methods: &logged_methods,
            git_source_cache: &pnpm_git_fetcher::GitSourceCache::default(),
        },
        http_client: &pnpm_network::ThrottledClient::default(),
        store_index: None,
        store_index_writer: None,
        prefetched_cas_paths: None,
        progress_reported: None,
        tarball_mem_cache,
        verified_files_cache: &verified_files_cache,
        skipped: &skipped,
        include_optional_dependencies: true,
        runtime_platform_selector: &host_platform_selector(),
        custom_fetcher_session: Some(session),
        defer_link: false,
        link_concurrency_probe: None,
    }
    .run::<pnpm_reporter::SilentReporter>(&package_key, metadata, &snapshot)
    .await
}

/// A minimal runtime archive: one executable at `<top>/bin/node` and no
/// `package.json`. Real Node.js / Bun / Deno archives likewise ship no
/// manifest of their own; the synthesized `bin` comes from the
/// resolution, not the archive, so the payload is deliberately trivial.
fn build_runtime_tarball_fixture() -> Vec<u8> {
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;

    let script = b"#!/bin/sh\necho v22.0.0\n";
    let mut tar_builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(script.len() as u64);
    header.set_mode(0o755);
    tar_builder
        .append_data(&mut header, "node-v22.0.0-fixture/bin/node", &script[..])
        .expect("append the fixture bin entry");
    let tar_bytes = tar_builder.into_inner().expect("finalize the fixture tar");

    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(&tar_bytes).expect("gzip the fixture tar");
    gz.finish().expect("finish the gzip stream")
}

fn custom_resolution_metadata(resolution_type: &str) -> pnpm_lockfile::PackageMetadata {
    let mut extra = serde_json::Map::new();
    extra.insert(
        "url".to_string(),
        serde_json::Value::String("https://example.test/foo-1.0.0.tgz".to_string()),
    );
    let mut metadata = registry_metadata();
    metadata.resolution = LockfileResolution::Custom(pnpm_lockfile::CustomResolution {
        resolution_type: resolution_type.to_string().try_into().expect("custom type tag"),
        extra,
    });
    metadata
}
