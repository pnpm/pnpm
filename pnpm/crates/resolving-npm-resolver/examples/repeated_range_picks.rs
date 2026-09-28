//! Repeated version selection, including the cached minimum-release-age view.
use core::hint::black_box;
use pnpm_registry::{MirrorFile, Package, PackageVersions};
use pnpm_resolving_npm_resolver::{
    PickPackageFromMetaOptions, RegistryPackageSpec, RegistryPackageSpecType,
    pick_package_from_meta, pick_version_by_version_range,
};
use std::{io::Write, time::Instant};

fn metadata(file_backed: bool) -> Package {
    let mut releases = serde_json::Map::new();
    let mut time = serde_json::Map::new();
    let mut bytes = Vec::new();
    let mut spans = Vec::new();
    for i in 0..1000 {
        let version = format!("1.0.{i}");
        let release = serde_json::json!({
            "name": "probe", "version": version,
            "deprecated": if i > 900 { serde_json::json!("use 2.x") } else { serde_json::json!(false) },
            "dist": { "tarball": "https://registry.example/probe.tgz" },
            "description": "x".repeat(2048),
        });
        let json = serde_json::to_vec(&release).unwrap();
        spans.push((version.clone(), bytes.len() as u64, json.len() as u32));
        bytes.extend(json);
        time.insert(version.clone(), serde_json::json!("2020-01-01T00:00:00.000Z"));
        releases.insert(version, release);
    }
    let mut package: Package = serde_json::from_value(serde_json::json!({
        "name": "probe", "dist-tags": { "latest": "1.0.999" }, "versions": releases, "time": time,
    }))
    .unwrap();
    if file_backed {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(&bytes).unwrap();
        let mirror = MirrorFile::try_hold(file, usize::MAX).unwrap();
        package.versions = PackageVersions::from_file_spans(&mirror, spans);
    }
    package
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let mode = args.get(1).map_or("raw", String::as_str);
    let picks: usize = args
        .get(2)
        .map_or(100, |value| value.parse().unwrap());
    let meta = metadata(mode.starts_with("file"));
    let opts = PickPackageFromMetaOptions {
        published_by: mode
            .ends_with("age")
            .then(|| "2025-01-01T00:00:00Z".parse().unwrap()),
        ..Default::default()
    };
    let specs: Vec<_> = (0..32)
        .map(|index| RegistryPackageSpec {
            name: "probe".into(),
            fetch_spec: format!("<=1.0.{}", 999 - index),
            spec_type: RegistryPackageSpecType::Range,
            revision: None,
            normalized_bare_specifier: None,
        })
        .collect();
    let start = Instant::now();
    for i in 0..picks {
        let picked = pick_package_from_meta(
            pick_version_by_version_range,
            &opts,
            black_box(&meta),
            &specs[i % 32],
        )
        .unwrap()
        .unwrap();
        assert_eq!(picked.version.to_string(), "1.0.900");
        black_box(picked);
    }
    println!("{:.6}", start.elapsed().as_secs_f64() * 1000.0);
}
