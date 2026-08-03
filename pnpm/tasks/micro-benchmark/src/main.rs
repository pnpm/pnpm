mod workspace_resolution;

use std::{fs, hint::black_box, path::Path, time::Duration};

use clap::Parser;
use criterion::{Criterion, Throughput};
use flate2::{Compression, write::GzEncoder};
use futures_util::future;
use mockito::ServerGuard;
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_registry::Package;
use pacquet_store_dir::StoreDir;
use pacquet_tarball::{DownloadTarballToStore, RetryOpts};
use pipe_trait::Pipe;
use project_root::get_project_root;
use ssri::Integrity;
use tar::{Builder, Header};
use tempfile::tempdir;

const BATCH_TARBALL_COUNT: usize = 256;
const BATCH_FILES_PER_TARBALL: usize = 64;

#[derive(Debug, Parser)]
struct CliArgs {
    #[clap(long, conflicts_with = "full_workspace_resolution")]
    save_baseline: Option<String>,

    /// Run each full-size workspace resolution shape once and print its
    /// timing instead of running the criterion suite. This size models a
    /// 331-importer workspace and has no statistical harness, so it takes
    /// no baseline: a regressed run can take minutes and several GiB.
    #[clap(long)]
    full_workspace_resolution: bool,
}

fn bench_tarball(criterion: &mut Criterion, server: &mut ServerGuard, fixtures_folder: &Path) {
    let mut group = criterion.benchmark_group("tarball");
    let file = fs::read(fixtures_folder.join("@fastify+error-3.3.0.tgz")).unwrap();
    server.mock("GET", "/@fastify+error-3.3.0.tgz").with_status(201).with_body(&file).create();

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();

    let url = &format!("{0}/@fastify+error-3.3.0.tgz", server.url());
    let package_integrity: Integrity = "sha512-dj7vjIn1Ar8sVXj2yAXiMNCJDmS9MQ9XMlIecX2dIzzhjSHCyKo4DdXjXMs7wKW2kj6yvVRSpuQjOZ3YLrh56w==".parse().expect("parse integrity string");

    group.throughput(Throughput::Bytes(file.len() as u64));
    group.bench_function("download_dependency", |bencher| {
        bencher.to_async(&rt).iter(|| async {
            // NOTE: the tempdir is being leaked, meaning the cleanup would be postponed until the end of the benchmark
            let dir = tempdir().unwrap();
            let store_dir =
                dir.path().to_path_buf().pipe(StoreDir::from).pipe(Box::new).pipe(Box::leak);
            let http_client = ThrottledClient::new_for_installs();

            let cas_map = DownloadTarballToStore {
                http_client: &http_client,
                store_dir,
                store_index: None,
                store_index_writer: None,
                verify_store_integrity: true,
                verified_files_cache: pacquet_store_dir::SharedVerifiedFilesCache::default(),
                package_integrity: Some(&package_integrity),
                package_unpacked_size: Some(16697),
                package_file_count: None,
                package_url: url,
                package_id: "fast-querystring@1.0.0",
                requester: "",
                prefetched_cas_paths: None,
                retry_opts: RetryOpts::default(),
                auth_headers: &AuthHeaders::default(),
                ignore_file_pattern: None,
                offline: false,
                progress_reported: None,
                append_manifest: None,
            }
            .run_without_mem_cache::<pacquet_reporter::SilentReporter>()
            .await
            .unwrap();
            cas_map.len()
        });
    });

    group.finish();
}

fn bench_concurrent_tarballs(criterion: &mut Criterion, server: &mut ServerGuard) {
    let packages = (0..BATCH_TARBALL_COUNT)
        .map(|package_index| {
            let tarball = create_benchmark_tarball(package_index);
            let path = format!("/batch-package-{package_index}.tgz");
            server.mock("GET", path.as_str()).with_status(200).with_body(tarball.clone()).create();
            BatchPackage {
                id: format!("batch-package-{package_index}@1.0.0"),
                integrity: Integrity::from(tarball.as_slice()),
                unpacked_size: BATCH_FILES_PER_TARBALL * benchmark_file(package_index, 0).len(),
                url: format!("{}{path}", server.url()),
            }
        })
        .collect::<Vec<_>>();

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let mut group = criterion.benchmark_group("tarball_batch");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(20));
    group.throughput(Throughput::Elements(
        u64::try_from(BATCH_TARBALL_COUNT * BATCH_FILES_PER_TARBALL)
            .expect("benchmark file count fits u64"),
    ));
    group.bench_function("cold_store_many_medium_tarballs", |bencher| {
        bencher.to_async(&rt).iter(|| async {
            let dir = tempdir().unwrap();
            let store_dir =
                dir.path().to_path_buf().pipe(StoreDir::from).pipe(Box::new).pipe(Box::leak);
            let http_client = ThrottledClient::new_for_installs();
            future::try_join_all(packages.iter().map(|package| async {
                let auth_headers = AuthHeaders::default();
                DownloadTarballToStore {
                    http_client: &http_client,
                    store_dir,
                    store_index: None,
                    store_index_writer: None,
                    verify_store_integrity: true,
                    verified_files_cache: pacquet_store_dir::SharedVerifiedFilesCache::default(),
                    package_integrity: Some(&package.integrity),
                    package_unpacked_size: Some(package.unpacked_size),
                    package_file_count: Some(BATCH_FILES_PER_TARBALL),
                    package_url: &package.url,
                    package_id: &package.id,
                    requester: "",
                    prefetched_cas_paths: None,
                    retry_opts: RetryOpts::default(),
                    auth_headers: &auth_headers,
                    ignore_file_pattern: None,
                    offline: false,
                    progress_reported: None,
                    append_manifest: None,
                }
                .run_without_mem_cache::<pacquet_reporter::SilentReporter>()
                .await
            }))
            .await
            .unwrap()
            .iter()
            .map(std::collections::HashMap::len)
            .sum::<usize>()
        });
    });
    group.finish();
}

struct BatchPackage {
    id: String,
    integrity: Integrity,
    unpacked_size: usize,
    url: String,
}

fn create_benchmark_tarball(package_index: usize) -> Vec<u8> {
    let encoder = GzEncoder::new(Vec::new(), Compression::fast());
    let mut archive = Builder::new(encoder);
    for file_index in 0..BATCH_FILES_PER_TARBALL {
        let content = benchmark_file(package_index, file_index);
        let mut header = Header::new_gnu();
        header.set_path(format!("package/files/file-{file_index}.txt")).unwrap();
        header.set_size(u64::try_from(content.len()).expect("benchmark file size fits u64"));
        header.set_mode(0o644);
        header.set_cksum();
        archive.append(&header, content.as_slice()).unwrap();
    }
    let encoder = archive.into_inner().unwrap();
    encoder.finish().unwrap()
}

fn benchmark_file(package_index: usize, file_index: usize) -> Vec<u8> {
    let prefix = format!("package={package_index};file={file_index};").into_bytes();
    let mut content =
        vec![u8::try_from((package_index + file_index) % 251).expect("value is below 251"); 128];
    content[..prefix.len()].copy_from_slice(&prefix);
    content
}

/// Isolate pacquet's resolve-time metadata parse: deserialize a registry
/// packument into [`Package`], then pick and hydrate a version — the CPU
/// `Package::fetch_from_registry` pays on the resolve hot path, minus the
/// network. `PackageVersions` captures each version as a raw fragment and
/// hydrates lazily, so this also guards that optimization: a regression that
/// eagerly hydrates every version would surface here as a parse blowup.
fn bench_packument(criterion: &mut Criterion, bytes: &[u8]) {
    let mut group = criterion.benchmark_group("packument");
    group.throughput(Throughput::Bytes(bytes.len() as u64));
    group.bench_function("parse", |bencher| {
        bencher.iter(|| {
            let package: Package = serde_json::from_slice(black_box(bytes)).unwrap();
            let latest = package.dist_tag("latest").expect("lodash lists a `latest` dist-tag");
            let manifest = package.versions.get(latest).expect("the `latest` manifest hydrates");
            black_box(manifest)
        });
    });
    group.finish();
}

/// Isolate the lockfile-parse sink
/// ([`pacquet_lockfile::Lockfile::load_wanted_from_dir`], `serde-saphyr`). The
/// per-iteration file read is page-cache-warm after the first pass, so the
/// 12k-line YAML parse dominates the measurement.
fn bench_lockfile(criterion: &mut Criterion, dir: &Path) {
    assert!(
        pacquet_lockfile::Lockfile::load_wanted_from_dir(dir).unwrap().is_some(),
        "fixture lockfile must parse to Some, else the bench measures nothing",
    );
    let bytes = fs::metadata(dir.join(pacquet_lockfile::Lockfile::FILE_NAME)).unwrap().len();
    let mut group = criterion.benchmark_group("lockfile");
    group.throughput(Throughput::Bytes(bytes));
    group.bench_function("parse_pnpm_lock", |bencher| {
        bencher.iter(|| {
            let lockfile =
                pacquet_lockfile::Lockfile::load_wanted_from_dir(black_box(dir)).unwrap();
            black_box(lockfile.is_some())
        });
    });
    group.finish();
}

pub fn main() -> Result<(), String> {
    let CliArgs { save_baseline, full_workspace_resolution } = CliArgs::parse();
    if full_workspace_resolution {
        workspace_resolution::run_full_workspace_resolution();
        return Ok(());
    }
    let mut server = mockito::Server::new();
    let root = get_project_root().unwrap();
    let fixtures_folder = root.join("pnpm/tasks/micro-benchmark/fixtures");

    let mut criterion = Criterion::default().without_plots();
    if let Some(baseline) = save_baseline {
        criterion = criterion.save_baseline(baseline);
    }

    let packument = fixtures_folder.join("lodash.json").pipe(fs::read).unwrap();
    let lockfile_dir = root.join("pnpm/tasks/integrated-benchmark/src/fixtures");

    bench_tarball(&mut criterion, &mut server, &fixtures_folder);
    bench_concurrent_tarballs(&mut criterion, &mut server);
    bench_packument(&mut criterion, &packument);
    bench_lockfile(&mut criterion, &lockfile_dir);
    workspace_resolution::bench_workspace_resolution(&mut criterion);

    Ok(())
}
