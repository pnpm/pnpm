//! Repeated range picks against one package's metadata, the way a resolve
//! picks the same package for many dependents. The newest releases are
//! deprecated, so every pick probes the deprecation status of the versions
//! it passes over before settling on the newest non-deprecated one. The
//! raw-fragment and file-backed [`PackageVersions`] shapes are measured
//! separately because a file-backed probe also pays a mirror read.

use criterion::{Criterion, Throughput};
use pnpm_registry::{MirrorFile, Package, PackageVersions};
use pnpm_resolving_npm_resolver::{
    PickPackageFromMetaOptions, RegistryPackageSpec, RegistryPackageSpecType,
    pick_package_from_meta, pick_version_by_version_range,
};
use std::{hint::black_box, io::Write};

const RELEASE_COUNT: usize = 1_000;
const FIRST_DEPRECATED: usize = 901;
const RANGE_COUNT: usize = 32;

pub fn bench_version_pick(criterion: &mut Criterion) {
    let specs = range_specs();
    let opts = PickPackageFromMetaOptions::default();
    let mut group = criterion.benchmark_group("version_pick");
    group.throughput(Throughput::Elements(RANGE_COUNT as u64));
    for (name, file_backed) in [("repeated_ranges_raw", false), ("repeated_ranges_file", true)] {
        let meta = synthetic_package(file_backed);
        group.bench_function(name, |bencher| {
            bencher.iter(|| pick_every_range(&meta, &specs, &opts));
        });
    }
    group.finish();
}

fn pick_every_range(
    meta: &Package,
    specs: &[RegistryPackageSpec],
    opts: &PickPackageFromMetaOptions,
) {
    for spec in specs {
        let picked =
            pick_package_from_meta(pick_version_by_version_range, opts, black_box(meta), spec)
                .unwrap()
                .expect("a non-deprecated release satisfies every range");
        assert_eq!(picked.version.to_string(), "1.0.900");
        black_box(picked);
    }
}

fn range_specs() -> Vec<RegistryPackageSpec> {
    (0..RANGE_COUNT)
        .map(|index| RegistryPackageSpec {
            name: "probe".into(),
            fetch_spec: format!("<=1.0.{}", RELEASE_COUNT - 1 - index),
            spec_type: RegistryPackageSpecType::Range,
            revision: None,
            normalized_bare_specifier: None,
        })
        .collect()
}

fn synthetic_package(file_backed: bool) -> Package {
    let mut releases = serde_json::Map::new();
    let mut fragments = Vec::new();
    let mut spans = Vec::new();
    for index in 0..RELEASE_COUNT {
        let version = format!("1.0.{index}");
        let deprecated = if index >= FIRST_DEPRECATED {
            serde_json::json!("use 2.x")
        } else {
            serde_json::json!(false)
        };
        let release = serde_json::json!({
            "name": "probe",
            "version": version,
            "deprecated": deprecated,
            "dist": { "tarball": "https://registry.example/probe.tgz" },
            "description": "x".repeat(2048),
        });
        let json = serde_json::to_vec(&release).unwrap();
        let len = u32::try_from(json.len()).expect("fragment length fits u32");
        spans.push((version.clone(), fragments.len() as u64, len));
        fragments.extend(json);
        releases.insert(version, release);
    }
    let mut package: Package = serde_json::from_value(serde_json::json!({
        "name": "probe",
        "dist-tags": { "latest": format!("1.0.{}", RELEASE_COUNT - 1) },
        "versions": releases,
    }))
    .unwrap();
    if file_backed {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(&fragments).unwrap();
        let mirror = MirrorFile::try_hold(file, usize::MAX).unwrap();
        package.versions = PackageVersions::from_file_spans(&mirror, spans);
    }
    package
}
