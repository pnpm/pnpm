use std::{hint::black_box, io::Write};

use criterion::{BatchSize, BenchmarkGroup, Criterion, Throughput, measurement::WallTime};
use pnpm_registry::{MirrorFile, PackageVersions, VersionTrustMetadata};
use serde_json::value::RawValue;

const RELEASE_COUNT: usize = 1_000;
const SCAN_COUNT: usize = 32;

pub fn bench_trust_history(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("trust_history");
    group.sample_size(20);
    for (source, file_backed) in [("raw", false), ("file", true)] {
        let versions = synthetic_versions(file_backed);
        bench_source(&mut group, source, &versions);
        group.throughput(Throughput::Elements((RELEASE_COUNT * SCAN_COUNT) as u64));
        group.bench_function(format!("{source}/uncached/fallback"), |bencher| {
            bencher.iter(|| scan_uncached(&versions));
        });
    }
    group.finish();
}

fn bench_source(
    group: &mut BenchmarkGroup<'_, WallTime>,
    source: &str,
    versions: &PackageVersions,
) {
    for (name, compact, scans) in [
        ("compact/cold", true, 1),
        ("compact/fallback", true, SCAN_COUNT),
        ("full/cold", false, 1),
        ("full/fallback", false, SCAN_COUNT),
    ] {
        group.throughput(Throughput::Elements((RELEASE_COUNT * scans) as u64));
        group.bench_function(format!("{source}/{name}"), |bencher| {
            bencher.iter_batched_ref(
                || versions.clone(),
                |metadata| scan_history(metadata, compact, scans),
                BatchSize::SmallInput,
            );
        });
    }
}

fn scan_history(versions: &PackageVersions, compact: bool, scans: usize) {
    for _ in 0..scans {
        for version in versions.keys() {
            if compact {
                black_box(versions.trust_metadata(version).expect("compact trust decodes"));
            } else {
                black_box(versions.get(version).expect("full manifest decodes"));
            }
        }
    }
}

fn scan_uncached(versions: &PackageVersions) {
    for _ in 0..SCAN_COUNT {
        for (_, json) in versions.fragments() {
            black_box(serde_json::from_str::<VersionTrustMetadata>(&json).unwrap());
        }
    }
}

fn synthetic_versions(file_backed: bool) -> PackageVersions {
    let mut fragments = Vec::new();
    let mut bytes = Vec::new();
    let mut spans = Vec::new();
    for index in 0..RELEASE_COUNT {
        let version = format!("1.0.{index}");
        let json = serde_json::to_string(&serde_json::json!({
            "name": "probe",
            "version": version,
            "_npmUser": {"name": "maintainer", "trustedPublisher": true},
            "dist": {
                "tarball": "https://registry.example/probe.tgz",
                "attestations": {"provenance": true}
            },
            "description": "x".repeat(2048),
            "dependencies": {"alpha": "^1", "beta": "^2", "gamma": "^3"},
        }))
        .unwrap();
        spans.push((version.clone(), bytes.len() as u64, u32::try_from(json.len()).unwrap()));
        bytes.extend_from_slice(json.as_bytes());
        fragments.push((version, RawValue::from_string(json).unwrap()));
    }
    if !file_backed {
        return PackageVersions::from_buffered_mirror_fragments(fragments);
    }
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&bytes).unwrap();
    let mirror = MirrorFile::try_hold(file, usize::MAX).unwrap();
    PackageVersions::from_file_spans(&mirror, spans)
}
