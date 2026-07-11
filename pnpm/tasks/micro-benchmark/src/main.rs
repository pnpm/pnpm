use std::{fs, hint::black_box, path::Path};

use clap::Parser;
use criterion::{Criterion, Throughput};
use mockito::ServerGuard;
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_store_dir::StoreDir;
use pacquet_tarball::{DownloadTarballToStore, RetryOpts};
use pipe_trait::Pipe;
use project_root::get_project_root;
use ssri::{Algorithm, Integrity, IntegrityOpts};
use tempfile::tempdir;
use zune_inflate::{DeflateDecoder, DeflateOptions};

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

/// Isolate the gzip-inflate CPU sink, mirroring
/// `pacquet_tarball::decompress_gzip` (`zune-inflate`) with none of the
/// HTTP / tokio / filesystem work the combined `tarball` bench also pays.
/// The `DeflateOptions` match that call site so the measured code path is the
/// same one taken on every real download.
fn bench_inflate(criterion: &mut Criterion, gz: &[u8]) {
    let decoded_len = DeflateDecoder::new(gz).decode_gzip().unwrap().len();
    let mut group = criterion.benchmark_group("inflate");
    group.throughput(Throughput::Bytes(decoded_len as u64));
    group.bench_function("gunzip_tarball", |bencher| {
        bencher.iter(|| {
            let options = DeflateOptions::default().set_confirm_checksum(false);
            let out =
                DeflateDecoder::new_with_options(black_box(gz), options).decode_gzip().unwrap();
            black_box(out.len())
        });
    });
    group.finish();
}

/// Isolate the SHA-512 integrity sink (`ssri` -> `sha2`), the hottest hash on
/// the install path: whole-tarball verify plus per-file CAS hashing. `compute`
/// mirrors [`IntegrityOpts`]-based hashing; `check` mirrors
/// [`Integrity::check`], the verify taken on every download under the default
/// `verify-store-integrity=true`.
fn bench_sha512(criterion: &mut Criterion, data: &[u8]) {
    let integrity = {
        let mut opts = IntegrityOpts::new().algorithm(Algorithm::Sha512);
        opts.input(data);
        opts.result()
    };
    let mut group = criterion.benchmark_group("integrity");
    group.throughput(Throughput::Bytes(data.len() as u64));
    group.bench_function("sha512_compute", |bencher| {
        bencher.iter(|| {
            let mut opts = IntegrityOpts::new().algorithm(Algorithm::Sha512);
            opts.input(black_box(data));
            black_box(opts.result())
        });
    });
    group.bench_function("sha512_check", |bencher| {
        bencher.iter(|| black_box(integrity.check(black_box(data)).unwrap()));
    });
    group.finish();
}

/// Isolate the `serde_json` packument-parse sink — resolution's big JSON
/// deserialize (`pacquet_registry::Package::fetch_from_registry`). Parsing
/// into [`serde_json::Value`] exercises the same byte-cruncher without
/// pinning the bench to the registry types' evolving shape.
fn bench_json(criterion: &mut Criterion, bytes: &[u8]) {
    let mut group = criterion.benchmark_group("json");
    group.throughput(Throughput::Bytes(bytes.len() as u64));
    group.bench_function("packument_parse_value", |bencher| {
        bencher.iter(|| {
            let value: serde_json::Value = serde_json::from_slice(black_box(bytes)).unwrap();
            black_box(value.is_object())
        });
    });
    group.finish();
}

/// Isolate the lockfile-parse sink
/// ([`pacquet_lockfile::Lockfile::load_wanted_from_dir`], `serde-saphyr`). The
/// per-iteration file read is page-cache-warm after the first pass, so the
/// 12k-line YAML parse dominates the measurement.
fn bench_lockfile(criterion: &mut Criterion, dir: &Path) {
    let bytes = fs::read(dir.join("pnpm-lock.yaml")).unwrap().len();
    let mut group = criterion.benchmark_group("lockfile");
    group.throughput(Throughput::Bytes(bytes as u64));
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

    let tarball = fs::read(fixtures_folder.join("@fastify+error-3.3.0.tgz")).unwrap();
    // A committed snapshot of lodash's abbreviated packument
    // (`application/vnd.npm.install-v1+json`) — the metadata format pacquet
    // fetches during resolution. Refresh by saving
    // <https://registry.npmjs.org/lodash> requested with that `accept` header.
    let packument = fs::read(fixtures_folder.join("lodash.json")).unwrap();
    // The integrated-benchmark's real 12k-line `pnpm-lock.yaml`.
    let lockfile_dir = root.join("pnpm/tasks/integrated-benchmark/src/fixtures");

    bench_tarball(&mut criterion, &mut server, &fixtures_folder);
    bench_inflate(&mut criterion, &tarball);
    bench_sha512(&mut criterion, &tarball);
    bench_json(&mut criterion, &packument);
    bench_lockfile(&mut criterion, &lockfile_dir);

    Ok(())
}
