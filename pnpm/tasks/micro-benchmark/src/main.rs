use std::{fs, hint::black_box, path::Path};

use clap::Parser;
use criterion::{Criterion, Throughput};
use mockito::ServerGuard;
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_registry::Package;
use pacquet_store_dir::StoreDir;
use pacquet_tarball::{DownloadTarballToStore, RetryOpts};
use pipe_trait::Pipe;
use project_root::get_project_root;
use ssri::Integrity;
use tempfile::tempdir;

#[derive(Debug, Parser)]
struct CliArgs {
    #[clap(long)]
    save_baseline: Option<String>,
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
                package_integrity: &package_integrity,
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
    let mut server = mockito::Server::new();
    let CliArgs { save_baseline } = CliArgs::parse();
    let root = get_project_root().unwrap();
    let fixtures_folder = root.join("pnpm/tasks/micro-benchmark/fixtures");

    let mut criterion = Criterion::default().without_plots();
    if let Some(baseline) = save_baseline {
        criterion = criterion.save_baseline(baseline);
    }

    let packument = fs::read(fixtures_folder.join("lodash.json")).unwrap();
    let lockfile_dir = root.join("pnpm/tasks/integrated-benchmark/src/fixtures");

    bench_tarball(&mut criterion, &mut server, &fixtures_folder);
    bench_packument(&mut criterion, &packument);
    bench_lockfile(&mut criterion, &lockfile_dir);

    Ok(())
}
