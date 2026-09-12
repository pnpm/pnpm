mod cas_paths;

mod integrity;

mod store;

mod installation;

mod reporting;

use super::CreateVirtualStore;
use pnpm_lockfile::{
    GitResolution, LockfileEntries, LockfileResolution, PackageKey, PackageMetadata, PkgName,
    PkgVerPeer, RegistryResolution, SnapshotDepRef, SnapshotEntry, TarballResolution,
};
use pnpm_reporter::SilentReporter;
use std::{collections::HashMap, fs, sync::atomic::AtomicU8};

fn name(text: &str) -> PkgName {
    PkgName::parse(text).expect("parse pkg name")
}

fn metadata_with_integrity(integrity: &str) -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: integrity.parse().expect("parse integrity"),
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

fn snapshot_with_dep(child: &str, ref_str: &str) -> SnapshotEntry {
    let dep_ref: SnapshotDepRef = ref_str.parse().expect("parse SnapshotDepRef");
    SnapshotEntry {
        dependencies: Some(HashMap::from([(name(child), dep_ref)])),
        ..Default::default()
    }
}

fn dep_map(children: &[&str]) -> Option<HashMap<PkgName, SnapshotDepRef>> {
    if children.is_empty() {
        return None;
    }
    // The ref value is irrelevant to `removed_child_aliases`; only the
    // alias keys matter. A bare version is the simplest valid ref.
    Some(children.iter().map(|child| (name(child), "1.0.0".parse().expect("ref"))).collect())
}

fn snapshot(deps: &[&str], optional: &[&str]) -> SnapshotEntry {
    SnapshotEntry {
        dependencies: dep_map(deps),
        optional_dependencies: dep_map(optional),
        ..Default::default()
    }
}

const DUMMY_SHA512: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

/// A one-package lockfile whose store row and CAS blobs are seeded under
/// the install's own store, for the warm-restore integrity scenarios.
struct SeededStoreInstall {
    _root: tempfile::TempDir,
    workspace_root: std::path::PathBuf,
    config: &'static pnpm_config::Config,
    package_key: PackageKey,
    snapshots: HashMap<PackageKey, SnapshotEntry>,
    packages: HashMap<PackageKey, PackageMetadata>,
    /// The CAS blob of the package's `index.js`.
    body_blob: std::path::PathBuf,
}

impl SeededStoreInstall {
    /// `build_output` is the content of a `build/output.js` a cached
    /// build added, recorded in the row's side-effects cache under one
    /// dep-state cache key. `None` seeds a row without a side-effects
    /// cache.
    fn new(build_output: Option<&[u8]>) -> Self {
        use pnpm_config::{Config, PackageImportMethod};
        use pnpm_store_dir::{
            CafsFileInfo, PackageFilesIndex, SideEffectsDiff, StoreIndex, store_index_key,
        };

        let root = tempfile::tempdir().expect("create temp dir");
        let workspace_root = root.path().join("workspace");
        fs::create_dir_all(&workspace_root).expect("create workspace root");
        let modules_dir = workspace_root.join("node_modules");

        let mut config = Config::new();
        config.registry = "https://registry.test".to_string();
        config.store_dir = root.path().join("store").into();
        config.modules_dir = modules_dir.clone();
        config.virtual_store_dir = modules_dir.join(".pacquet");
        config.enable_global_virtual_store = true;
        config.global_virtual_store_dir = root.path().join("links");
        config.package_import_method = PackageImportMethod::Copy;
        config.offline = true;

        let package_key = key("seeded", "1.0.0");
        let mut files = HashMap::new();
        let mut body_blob = None;
        for (path, content) in [
            ("package.json", br#"{"name":"seeded","version":"1.0.0"}"#.as_slice()),
            ("index.js", b"module.exports = true\n".as_slice()),
        ] {
            let (blob, digest) =
                config.store_dir.write_cas_file(content, false).expect("write package file");
            if path == "index.js" {
                body_blob = Some(blob);
            }
            files.insert(
                path.to_string(),
                CafsFileInfo {
                    digest: format!("{digest:x}"),
                    mode: 0o644,
                    size: content.len() as u64,
                    checked_at: None,
                },
            );
        }
        let side_effects = build_output.map(|content| {
            let (_, digest) =
                config.store_dir.write_cas_file(content, false).expect("write build output");
            let added = HashMap::from([(
                "build/output.js".to_string(),
                CafsFileInfo {
                    digest: format!("{digest:x}"),
                    mode: 0o644,
                    size: content.len() as u64,
                    checked_at: None,
                },
            )]);
            HashMap::from([(
                "linux-x64-node22".to_string(),
                SideEffectsDiff { added: Some(added), deleted: None, remote_origin: None },
            )])
        });
        StoreIndex::open_in(&config.store_dir)
            .expect("open store index")
            .set(
                &store_index_key(DUMMY_SHA512, &package_key.without_peer().pkg_id()),
                &PackageFilesIndex {
                    manifest: None,
                    requires_build: Some(false),
                    requires_prepare: None,
                    algo: "sha512".to_string(),
                    files,
                    side_effects,
                    remote_side_effects_quarantine: None,
                },
            )
            .expect("seed store index");

        SeededStoreInstall {
            _root: root,
            workspace_root,
            config: config.leak(),
            snapshots: HashMap::from([(package_key.clone(), SnapshotEntry::default())]),
            packages: HashMap::from([(
                package_key.without_peer(),
                metadata_with_integrity(DUMMY_SHA512),
            )]),
            package_key,
            body_blob: body_blob.expect("index.js blob"),
        }
    }

    async fn run(&self) -> Result<super::CreateVirtualStoreOutput, super::CreateVirtualStoreError> {
        use crate::{AllowBuildPolicy, SkippedSnapshots, VirtualStoreLayout};
        use pnpm_config::NodeLinker;
        use pnpm_store_dir::StoreIndexWriter;
        use pnpm_tarball::SharedReportedProgressKeys;

        let allow_build_policy = AllowBuildPolicy::default();
        let layout = VirtualStoreLayout::new(
            self.config,
            Some("linux-x64-node22"),
            Some(&self.snapshots),
            Some(&self.packages),
            Some(&allow_build_policy),
            None,
        );
        let skipped = SkippedSnapshots::new();
        let logged_methods = AtomicU8::new(0);
        let progress_reported = SharedReportedProgressKeys::default();
        let (store_index_writer, writer_task) = StoreIndexWriter::spawn(&self.config.store_dir);
        let requester = self.workspace_root.to_string_lossy().into_owned();

        let output = CreateVirtualStore {
            ctx: &crate::InstallContext {
                config: self.config,
                workspace_root: &self.workspace_root,
                requester: &requester,
                layout: &layout,
                node_linker: NodeLinker::Isolated,
                allow_build_policy: &allow_build_policy,
                link_options: &pnpm_cmd_shim::LinkBinsOptions::default(),
                logged_methods: &logged_methods,
                git_source_cache: &pnpm_git_fetcher::GitSourceCache::default(),
                dir_clone_cache: None,
            },
            http_client: &pnpm_network::ThrottledClient::default(),
            entries: LockfileEntries {
                packages: Some(&self.packages),
                snapshots: Some(&self.snapshots),
            },
            current_entries: LockfileEntries::default(),
            store_index_writer: &store_index_writer,
            store_context: None,
            cas_prefetch: None,
            skipped: &skipped,
            include_optional_dependencies: true,
            supported_architectures: None,
            dir_clone_cache: None,
            progress_reported: &progress_reported,
            tarball_mem_cache: None,
            custom_fetcher_session: None,
            planned_canonical_fetches: None,
            link_concurrency_probe: None,
        }
        .run::<SilentReporter>()
        .await;

        drop(store_index_writer);
        writer_task.await.expect("join store-index writer").expect("flush store-index writer");
        output
    }
}

fn ver(text: &str) -> PkgVerPeer {
    text.parse().expect("parse PkgVerPeer")
}

fn key(name_text: &str, version: &str) -> PackageKey {
    PackageKey::new(name(name_text), ver(version))
}

fn git_metadata() -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Git(GitResolution {
            repo: "https://github.com/ksxnodemodules/ts-pipe-compose.git".to_string(),
            commit: "e63c09e460269b0c535e4c34debf69bb91d57b22".to_string(),
            integrity: None,
            path: None,
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

fn git_hosted_tarball_metadata() -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: "https://codeload.github.com/foo/bar/tar.gz/f43f6a1cefff47fb361c88cf4b943fdbcaafe540"
                .to_string(),
            integrity: None,
            revision: None,
            git_hosted: Some(true),
            path: None,
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

fn tarball_metadata_without_integrity() -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz".to_string(),
            integrity: None,
            revision: None,
            git_hosted: None,
            path: None,
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

/// Helpers for the `group_slots_by_dir` tests: a GVS layout over the
/// given snapshots/metadata, scoped to a lockfile dir so directory
/// resolutions hash the way a real project's do.
fn gvs_layout(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
    lockfile_dir: &std::path::Path,
) -> crate::VirtualStoreLayout {
    let mut config = pnpm_config::Config::new();
    config.enable_global_virtual_store = true;
    config.virtual_store_dir = std::path::PathBuf::from("/tmp/proj/node_modules/.pnpm");
    config.global_virtual_store_dir = std::path::PathBuf::from("/tmp/store/links");
    let config = config.leak();
    crate::VirtualStoreLayout::new(
        config,
        Some("linux-x64-node22"),
        Some(snapshots),
        Some(packages),
        None,
        Some(lockfile_dir),
    )
}

fn directory_metadata(directory: &str) -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Directory(pnpm_lockfile::DirectoryResolution {
            directory: directory.to_string(),
        }),
        version: Some("1.0.0".to_string()),
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

fn slot_link<'a>(
    snapshot_key: &'a PackageKey,
    snapshot: &'a SnapshotEntry,
    cas_paths: &'a HashMap<String, std::path::PathBuf>,
    removed_aliases: &'a [PkgName],
) -> crate::create_virtual_store::slot_linking::SlotLink<'a> {
    crate::create_virtual_store::slot_linking::SlotLink {
        snapshot_key,
        snapshot,
        cas_paths,
        warm_cache_key: None,
        source_is_mutable: true,
        force_import: false,
        needs_build_marker_source: None,
        dir_clone_cacheable: false,
        removed_aliases,
    }
}
